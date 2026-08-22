//! Replay transport: plays a recorded `.obcap` capture back as a port, for
//! hardware-free reproduction of real sessions.
//!
//! Every engine write is verified byte-for-byte against the capture's tx
//! records; a mismatch or a write past the end of the capture puts the
//! transport into a loud error state that surfaces through the session. rx
//! records are played as chunks with their original inter-record gaps, capped
//! so long real-world waits do not slow the replay down.

use crate::engine::transport::BoxedPort;
use anyhow::{anyhow, bail, Context};
use openbaud_core::hex;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};

/// Cap on the sleep between adjacent rx records: preserves the gap structure
/// framing depends on while compressing long real-world waits.
const MAX_GAP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Tx,
    Rx,
}

#[derive(Debug)]
struct Record {
    /// 1-based line number in the capture file, for error messages.
    line: usize,
    ts_ms: u64,
    dir: Dir,
    bytes: Vec<u8>,
}

/// Open a `.obcap` capture file as a replayable port.
pub fn open_replay(path: &Path) -> anyhow::Result<BoxedPort> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("cannot open capture file {path:?} for replay"))?;
    let records = parse_obcap(&content, path)?;

    let (client, server) = tokio::io::duplex(64 * 1024);
    let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    tokio::spawn(replay_task(server, records, Arc::clone(&error)));
    Ok(Box::new(ReplayTransport { inner: client, error }))
}

#[derive(serde::Deserialize)]
struct RawRecord {
    ts_ms: u64,
    dir: String,
    hex: String,
}

fn parse_obcap(content: &str, path: &Path) -> anyhow::Result<Vec<Record>> {
    let mut lines = content.lines().enumerate();
    let (_, header) = lines
        .next()
        .ok_or_else(|| anyhow!("capture file {path:?} is empty, expected an obcap header line"))?;
    let header: serde_json::Value = serde_json::from_str(header)
        .with_context(|| format!("capture file {path:?}: first line is not JSON"))?;
    if header.get("obcap").is_none() {
        bail!("capture file {path:?} is not an obcap capture (header has no \"obcap\" field)");
    }

    let mut records = Vec::new();
    for (i, line) in lines {
        if line.trim().is_empty() {
            continue;
        }
        let line_no = i + 1;
        let raw: RawRecord = serde_json::from_str(line).with_context(|| {
            format!("capture file {path:?} line {line_no}: not a valid obcap record")
        })?;
        let dir = match raw.dir.as_str() {
            "tx" => Dir::Tx,
            "rx" => Dir::Rx,
            other => bail!(
                "capture file {path:?} line {line_no}: dir is {other:?}, expected \"tx\" or \"rx\""
            ),
        };
        let bytes = hex::parse_hex(&raw.hex)
            .with_context(|| format!("capture file {path:?} line {line_no}: bad hex"))?;
        if bytes.is_empty() {
            continue; // zero-byte records carry nothing to verify or play
        }
        records.push(Record { line: line_no, ts_ms: raw.ts_ms, dir, bytes });
    }
    Ok(records)
}

/// The engine-facing half: a duplex stream plus a shared error slot the
/// background task fills on divergence/exhaustion. The slot is checked before
/// every read/write poll so the specific message — not a bare EOF — reaches
/// the session error.
struct ReplayTransport {
    inner: DuplexStream,
    error: Arc<Mutex<Option<String>>>,
}

impl ReplayTransport {
    fn pending_error(&self) -> Option<std::io::Error> {
        self.error.lock().unwrap().as_ref().map(|msg| std::io::Error::other(msg.clone()))
    }
}

impl AsyncRead for ReplayTransport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if let Some(e) = this.pending_error() {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for ReplayTransport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if let Some(e) = this.pending_error() {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut this.inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Drives the capture: verifies incoming tx bytes against the recorded tx
/// stream and plays the rx runs that follow each completed tx record. On
/// divergence or exhaustion the error slot is set *before* the server half is
/// dropped, so the woken reader sees the message rather than a bare EOF.
async fn replay_task(
    server: DuplexStream,
    records: Vec<Record>,
    error: Arc<Mutex<Option<String>>>,
) {
    let (mut read, mut write) = tokio::io::split(server);
    let fail = |msg: String| *error.lock().unwrap() = Some(msg);

    let mut idx = 0usize;
    // rx records before any tx play as soon as the port opens.
    if play_rx_run(&mut write, &records, &mut idx).await.is_err() {
        return; // engine side gone
    }

    let mut tx_off = 0usize; // byte offset into the current tx record
    let mut buf = [0u8; 4096];
    loop {
        let n = match read.read(&mut buf).await {
            Ok(0) | Err(_) => return, // engine closed its side
            Ok(n) => n,
        };
        let mut incoming = &buf[..n];
        // One engine write may span several consecutive tx records, or only
        // part of one: align on the raw byte stream.
        while !incoming.is_empty() {
            let Some(rec) = records.get(idx) else {
                fail(format!(
                    "capture exhausted: engine wrote {} but the capture has no tx records left",
                    hex::to_hex(incoming)
                ));
                return;
            };
            debug_assert_eq!(rec.dir, Dir::Tx, "cursor always rests on a tx record or the end");
            let expected = &rec.bytes[tx_off..];
            let take = incoming.len().min(expected.len());
            if incoming[..take] != expected[..take] {
                fail(format!(
                    "TX diverged from capture: expected {}, got {} (record at line {})",
                    hex::to_hex(expected),
                    hex::to_hex(incoming),
                    rec.line
                ));
                return;
            }
            tx_off += take;
            incoming = &incoming[take..];
            if tx_off == rec.bytes.len() {
                idx += 1;
                tx_off = 0;
                if play_rx_run(&mut write, &records, &mut idx).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Play the run of consecutive rx records at the cursor, sleeping
/// min(original gap, `MAX_GAP`) between adjacent records. Leaves the cursor
/// on the next non-rx record (or the end).
async fn play_rx_run(
    write: &mut tokio::io::WriteHalf<DuplexStream>,
    records: &[Record],
    idx: &mut usize,
) -> std::io::Result<()> {
    while let Some(rec) = records.get(*idx) {
        if rec.dir != Dir::Rx {
            break;
        }
        write.write_all(&rec.bytes).await?;
        write.flush().await?;
        *idx += 1;
        if let Some(next) = records.get(*idx) {
            if next.dir == Dir::Rx {
                let gap = Duration::from_millis(next.ts_ms.saturating_sub(rec.ts_ms));
                tokio::time::sleep(gap.min(MAX_GAP)).await;
            }
        }
    }
    Ok(())
}
