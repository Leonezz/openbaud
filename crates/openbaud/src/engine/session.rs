//! Port sessions: a background reader task feeds a bounded chunk buffer;
//! consumers (the `read` tool's deframing cursor, `request`'s matcher) drain
//! it independently. Buffer overflow is reported, never silent.

use crate::engine::capture::{CaptureStats, CaptureWriter};
use crate::engine::stream::SubscriptionSet;
use crate::engine::transport::BoxedPort;
use crate::engine::now_ms;
use anyhow::{anyhow, bail, Context};
use openbaud_core::framing::{Deframer, Framing, MatchRule, Matcher};
use openbaud_core::hex;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

/// Cap on buffered unread bytes per session; oldest chunks are dropped (and
/// counted, and reported to the consumer) beyond this.
const MAX_BUFFERED: usize = 1024 * 1024;

/// Polling granularity for waits; serial timescales make this negligible while
/// eliminating notify races entirely.
const POLL: Duration = Duration::from_millis(10);

struct Chunk {
    ts_ms: u64,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct ChunkBuf {
    chunks: VecDeque<Chunk>,
    base_seq: u64,
    buffered: usize,
    dropped_bytes: u64,
}

impl ChunkBuf {
    fn push(&mut self, chunk: Chunk) {
        self.buffered += chunk.bytes.len();
        self.chunks.push_back(chunk);
        while self.buffered > MAX_BUFFERED {
            let old = self.chunks.pop_front().expect("buffered > 0 implies chunks");
            self.buffered -= old.bytes.len();
            self.dropped_bytes += old.bytes.len() as u64;
            self.base_seq += 1;
        }
    }

    fn next_seq(&self) -> u64 {
        self.base_seq + self.chunks.len() as u64
    }

    /// Chunks with seq >= cursor; returns (chunks, new_cursor, skipped) where
    /// `skipped` is how many chunks were dropped to overflow past this
    /// cursor — data this consumer can never see. Non-zero skipped is loss
    /// the caller must attribute, never swallow.
    fn since(&self, cursor: u64) -> (Vec<(u64, Vec<u8>)>, u64, u64) {
        let start = cursor.max(self.base_seq);
        let out = self
            .chunks
            .iter()
            .skip((start - self.base_seq) as usize)
            .map(|c| (c.ts_ms, c.bytes.clone()))
            .collect();
        (out, self.next_seq(), start - cursor)
    }
}

struct Shared {
    buf: StdMutex<ChunkBuf>,
    notify: Notify,
    last_rx_ms: AtomicU64,
    /// Lifetime rx/tx byte totals, for `session_stats`.
    rx_bytes: AtomicU64,
    tx_bytes: AtomicU64,
    error: StdMutex<Option<String>>,
    capture: StdMutex<Option<CaptureWriter>>,
}

impl Shared {
    fn check_alive(&self) -> anyhow::Result<()> {
        if let Some(e) = self.error.lock().unwrap().as_ref() {
            bail!("session port failed: {e}");
        }
        Ok(())
    }

    fn capture_record(&self, dir: &str, ts_ms: u64, data: &[u8]) -> anyhow::Result<()> {
        if let Some(cap) = self.capture.lock().unwrap().as_mut() {
            cap.record(dir, ts_ms, data)?;
        }
        Ok(())
    }
}

struct ReadState {
    cursor: u64,
    seen_dropped: u64,
    deframer: Deframer,
    pending_since_ms: Option<u64>,
    /// Frames deframed but not yet delivered (overflow of a max_frames read).
    queued: Vec<Frame>,
}

pub struct Session {
    pub id: String,
    pub port_name: String,
    /// The framing the read cursor deframes with (a copy of what the deframer
    /// was built from), readable without touching the read-state lock.
    framing: Framing,
    /// Wall-clock ms when the session was spawned.
    opened_ms: u64,
    /// Transport settings the port was opened with, set by the opener once the
    /// open succeeded. `None` until then (e.g. CLI sessions that never set it).
    transport: StdMutex<Option<serde_json::Value>>,
    writer: Mutex<WriteHalf<BoxedPort>>,
    shared: Arc<Shared>,
    read_state: Mutex<ReadState>,
    request_lock: Mutex<()>,
    /// Per-consumer `stream_poll` subscriptions (engine in `stream.rs`).
    /// Deliberately disjoint from `read_state` — each subscription owns its
    /// own cursor, deframer and retention queue, so the `read` tool and the
    /// subscriptions can never steal frames from each other (risk ①). Living
    /// inside the session, they are all released when it closes (risk ③).
    pub(crate) subs: StdMutex<SubscriptionSet>,
    reader: JoinHandle<()>,
}

#[derive(Debug, serde::Serialize)]
pub struct Frame {
    pub ts_ms: u64,
    pub hex: String,
    pub text: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ReadResult {
    pub frames: Vec<Frame>,
    /// Bytes lost to buffer overflow since the previous read. Non-zero means
    /// the reader could not keep up; slow down or start a capture instead.
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_bytes: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Session {
    pub fn spawn(id: String, port_name: String, framing: Framing, port: BoxedPort) -> Arc<Self> {
        let (read_half, write_half) = tokio::io::split(port);
        let shared = Arc::new(Shared {
            buf: StdMutex::new(ChunkBuf::default()),
            notify: Notify::new(),
            last_rx_ms: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            error: StdMutex::new(None),
            capture: StdMutex::new(None),
        });
        let reader = tokio::spawn(reader_loop(read_half, Arc::clone(&shared)));
        Arc::new(Self {
            id,
            port_name,
            framing: framing.clone(),
            opened_ms: now_ms(),
            transport: StdMutex::new(None),
            writer: Mutex::new(write_half),
            shared,
            read_state: Mutex::new(ReadState {
                cursor: 0,
                seen_dropped: 0,
                deframer: Deframer::new(framing),
                pending_since_ms: None,
                queued: Vec::new(),
            }),
            request_lock: Mutex::new(()),
            subs: StdMutex::new(SubscriptionSet::default()),
            reader,
        })
    }

    /// The framing this session deframes with; each stream subscription
    /// builds its own `Deframer` from it.
    pub(crate) fn framing(&self) -> &Framing {
        &self.framing
    }

    /// Snapshot of buffered chunks with seq >= `cursor` plus the new cursor
    /// and how many chunks overflow already dropped past that cursor — the
    /// non-destructive multi-cursor read every consumer (read state, request
    /// matcher, stream subscriptions) drains through.
    pub(crate) fn chunks_since(&self, cursor: u64) -> (Vec<(u64, Vec<u8>)>, u64, u64) {
        self.shared.buf.lock().unwrap().since(cursor)
    }

    /// Loud check that the port behind this session is still alive; the seam
    /// `stream_poll` uses to mirror `read_frames`' dead-port rule.
    pub(crate) fn check_alive(&self) -> anyhow::Result<()> {
        self.shared.check_alive()
    }

    pub(crate) fn last_rx(&self) -> u64 {
        self.shared.last_rx_ms.load(Ordering::Relaxed)
    }

    /// The instantaneous session counters every `stream_poll` result folds in
    /// (`stream_stats` merged into `stream_poll` per roadmap §1.0): a strict
    /// subset of `stats()`, taken from the same short locks and atomics.
    pub fn stream_stats(&self) -> serde_json::Value {
        use serde_json::json;
        let (buffered, dropped_bytes) = {
            let buf = self.shared.buf.lock().unwrap();
            (buf.buffered, buf.dropped_bytes)
        };
        let last_rx = self.shared.last_rx_ms.load(Ordering::Relaxed);
        json!({
            "buffered": buffered,
            "dropped_bytes": dropped_bytes,
            "rx_bytes": self.shared.rx_bytes.load(Ordering::Relaxed),
            "tx_bytes": self.shared.tx_bytes.load(Ordering::Relaxed),
            // null, not 0, when nothing was ever received — same rule as stats().
            "last_rx_ms": if last_rx > 0 { json!(last_rx) } else { serde_json::Value::Null },
        })
    }

    /// Record the transport settings the port was opened with (for
    /// `session_stats`); called by the opener after a successful open.
    pub fn set_transport(&self, transport: serde_json::Value) {
        *self.transport.lock().unwrap() = Some(transport);
    }

    /// Live counters for `session_stats`: everything comes from short
    /// std-mutex locks or atomics, so a stats call can never block behind a
    /// long-running read.
    pub fn stats(&self) -> serde_json::Value {
        use serde_json::json;
        let (buffered, dropped_bytes, chunks_seen) = {
            let buf = self.shared.buf.lock().unwrap();
            (buf.buffered, buf.dropped_bytes, buf.next_seq())
        };
        let last_rx = self.shared.last_rx_ms.load(Ordering::Relaxed);
        let capture = match self.shared.capture.lock().unwrap().as_ref() {
            Some(writer) => {
                let (chunks, bytes) = writer.snapshot();
                json!({
                    "active": true,
                    "path": writer.path().display().to_string(),
                    "chunks": chunks,
                    "bytes": bytes,
                })
            }
            None => json!({ "active": false }),
        };
        let mut out = json!({
            "session_id": self.id,
            "port": self.port_name,
            "framing": format!("{:?}", self.framing),
            "opened_ms": self.opened_ms,
            "buffered": buffered,
            "dropped_bytes": dropped_bytes,
            "chunks_seen": chunks_seen,
            // null, not 0, when nothing was ever received: a zero epoch
            // timestamp would read as a real (1970) instant.
            "last_rx_ms": if last_rx > 0 { json!(last_rx) } else { serde_json::Value::Null },
            "rx_bytes": self.shared.rx_bytes.load(Ordering::Relaxed),
            "tx_bytes": self.shared.tx_bytes.load(Ordering::Relaxed),
            "capture": capture,
        });
        let obj = out.as_object_mut().expect("stats is an object");
        if let Some(e) = self.shared.error.lock().unwrap().as_ref() {
            obj.insert("error".to_string(), json!(e));
        }
        if let Some(t) = self.transport.lock().unwrap().as_ref() {
            obj.insert("transport".to_string(), t.clone());
        }
        out
    }

    pub async fn write_raw(&self, data: &[u8]) -> anyhow::Result<usize> {
        self.shared.check_alive()?;
        let mut writer = self.writer.lock().await;
        writer
            .write_all(data)
            .await
            .with_context(|| format!("write to {} failed", self.port_name))?;
        writer.flush().await.ok();
        self.shared.tx_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
        self.shared.capture_record("tx", now_ms(), data)?;
        Ok(data.len())
    }

    /// Cursor-based framed read for the `read` tool: returns as soon as at
    /// least one frame is available, or empty on timeout.
    pub async fn read_frames(&self, timeout_ms: u64, max_frames: usize) -> anyhow::Result<ReadResult> {
        let mut state = self.read_state.lock().await;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        let mut frames: Vec<Frame> = std::mem::take(&mut state.queued);
        let mut dropped = 0u64;

        loop {
            {
                let buf = self.shared.buf.lock().unwrap();
                let delta = buf.dropped_bytes - state.seen_dropped;
                if delta > 0 {
                    dropped += delta;
                    state.seen_dropped = buf.dropped_bytes;
                }
                // Byte loss for this cursor is already reported through the
                // `seen_dropped` delta above, so the skipped-chunk count is
                // redundant here.
                let (chunks, new_cursor, _skipped) = buf.since(state.cursor);
                state.cursor = new_cursor;
                drop(buf);
                for (ts, bytes) in chunks {
                    if state.pending_since_ms.is_none() {
                        state.pending_since_ms = Some(ts);
                    }
                    for frame in state.deframer.push(&bytes) {
                        frames.push(make_frame(ts, frame));
                    }
                    if state.deframer.pending_len() == 0 {
                        state.pending_since_ms = None;
                    }
                }
            }

            // Idle-gap framing: flush pending once the wire has been quiet.
            if let Framing::Idle { idle_ms } = state.deframer.framing() {
                let idle_ms = *idle_ms;
                if state.deframer.pending_len() > 0 {
                    let last = self.shared.last_rx_ms.load(Ordering::Relaxed);
                    if now_ms().saturating_sub(last) >= idle_ms {
                        if let Some(pending) = state.deframer.flush_pending() {
                            let ts = state.pending_since_ms.take().unwrap_or_else(now_ms);
                            frames.push(make_frame(ts, pending));
                        }
                    }
                }
            }

            if !frames.is_empty() || tokio::time::Instant::now() >= deadline {
                if frames.len() > max_frames {
                    state.queued = frames.split_off(max_frames);
                }
                // Surface a dead port only when there is nothing buffered left to deliver.
                if frames.is_empty() && dropped == 0 {
                    self.shared.check_alive()?;
                }
                return Ok(ReadResult { frames, dropped_bytes: dropped });
            }
            let _ = tokio::time::timeout(POLL, self.shared.notify.notified()).await;
        }
    }

    /// Write a frame and await its response per the match rule. Requests are
    /// serialized per session; response bytes are consumed from the point of
    /// transmission onward.
    pub async fn request(
        &self,
        tx: &[u8],
        rule: MatchRule,
        timeout_ms: u64,
    ) -> anyhow::Result<Vec<u8>> {
        let _guard = self.request_lock.lock().await;
        self.shared.check_alive()?;

        let mut cursor = self.shared.buf.lock().unwrap().next_seq();
        self.write_raw(tx).await?;

        let mut matcher = Matcher::new(rule.clone());
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            {
                let buf = self.shared.buf.lock().unwrap();
                // Cursor starts at next_seq, so nothing can have been dropped
                // past it within one request.
                let (chunks, new_cursor, _skipped) = buf.since(cursor);
                cursor = new_cursor;
                drop(buf);
                for (_ts, bytes) in chunks {
                    if let Some(frame) = matcher.push(&bytes) {
                        return Ok(frame);
                    }
                }
            }
            if let MatchRule::Idle { idle_ms } = &rule {
                let pending = matcher.take_pending();
                if !pending.is_empty() {
                    let last = self.shared.last_rx_ms.load(Ordering::Relaxed);
                    if now_ms().saturating_sub(last) >= *idle_ms {
                        return Ok(pending);
                    }
                    matcher.push(&pending);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                self.shared.check_alive()?;
                let partial = matcher.take_pending();
                if partial.is_empty() {
                    bail!("no response within {timeout_ms} ms (sent {})", hex::to_hex(tx));
                }
                bail!(
                    "response incomplete after {timeout_ms} ms: got {} which does not satisfy the match rule",
                    hex::to_hex(&partial)
                );
            }
            let _ = tokio::time::timeout(POLL, self.shared.notify.notified()).await;
        }
    }

    /// Classified request/response for declared commands: silence, timeout and
    /// protocol exception frames are results, not errors. `Err` is reserved
    /// for infrastructure failure (dead port, capture write failure, invalid
    /// exception rule).
    ///
    /// With `first_byte_ms` set, zero bytes within that window classifies as
    /// `Silence` early, without waiting out `timeout_ms`. While too few bytes
    /// have arrived to decide the exception predicate, the main rule stays
    /// active (a normal frame can be shorter than `when.at + 1` bytes); once
    /// the predicate hits, the frame is collected per `exception.match`.
    pub async fn request_classified(
        &self,
        tx: &[u8],
        rule: MatchRule,
        exception: Option<&openbaud_core::format::ExceptionSpec>,
        timeout_ms: u64,
        first_byte_ms: Option<u64>,
    ) -> anyhow::Result<openbaud_core::exec::RawOutcome> {
        use openbaud_core::exec::{exception_triggered, RawOutcome};

        // Resolve the exception frame rule up front: a bad spec is a caller
        // error, not a wire outcome.
        let exc_rule = match exception {
            Some(spec) => Some(spec.match_spec.to_rule("exception.match")?),
            None => None,
        };

        let _guard = self.request_lock.lock().await;
        self.shared.check_alive()?;

        let mut cursor = self.shared.buf.lock().unwrap().next_seq();
        self.write_raw(tx).await?;

        let start = tokio::time::Instant::now();
        let deadline = start + Duration::from_millis(timeout_ms);
        let first_byte_deadline = first_byte_ms.map(|ms| start + Duration::from_millis(ms));
        let mut buf: Vec<u8> = Vec::new();

        loop {
            {
                let shared_buf = self.shared.buf.lock().unwrap();
                // Cursor starts at next_seq, so nothing can have been dropped
                // past it within one request.
                let (chunks, new_cursor, _skipped) = shared_buf.since(cursor);
                cursor = new_cursor;
                drop(shared_buf);
                for (_ts, bytes) in chunks {
                    buf.extend_from_slice(&bytes);
                }
            }

            // Exception predicate: `None` (undecided) keeps the main rule.
            let is_exception = match exception {
                Some(spec) => exception_triggered(spec, &buf) == Some(true),
                None => false,
            };
            let active_rule = if is_exception {
                exc_rule.as_ref().expect("exc_rule is Some whenever exception is")
            } else {
                &rule
            };
            if let Some(frame) = self.try_complete(active_rule, &buf) {
                return Ok(RawOutcome::Frame { bytes: frame, is_exception });
            }

            let now = tokio::time::Instant::now();
            if buf.is_empty() {
                if let Some(fb) = first_byte_deadline {
                    if now >= fb {
                        self.shared.check_alive()?;
                        return Ok(RawOutcome::Silence);
                    }
                }
            }
            if now >= deadline {
                self.shared.check_alive()?;
                return Ok(if buf.is_empty() {
                    RawOutcome::Silence
                } else {
                    RawOutcome::Timeout { partial: buf }
                });
            }
            let _ = tokio::time::timeout(POLL, self.shared.notify.notified()).await;
        }
    }

    /// Whether the accumulated bytes complete a frame under `rule`. Length and
    /// delimiter rules rebuild a matcher over the full buffer (cheap at serial
    /// data rates); idle rules complete once the wire has been quiet.
    fn try_complete(&self, rule: &MatchRule, buf: &[u8]) -> Option<Vec<u8>> {
        if buf.is_empty() {
            return None;
        }
        match rule {
            MatchRule::Idle { idle_ms } => {
                let last = self.shared.last_rx_ms.load(Ordering::Relaxed);
                if now_ms().saturating_sub(last) >= *idle_ms {
                    Some(buf.to_vec())
                } else {
                    None
                }
            }
            _ => Matcher::new(rule.clone()).push(buf),
        }
    }

    pub fn capture_start(&self, path: &Path, note: Option<&str>) -> anyhow::Result<String> {
        let mut slot = self.shared.capture.lock().unwrap();
        if let Some(active) = slot.as_ref() {
            bail!("capture already active on this session: {:?}", active.path());
        }
        let writer = CaptureWriter::create(path, &self.id, &self.port_name, note, now_ms())?;
        let path = writer.path().display().to_string();
        *slot = Some(writer);
        Ok(path)
    }

    pub fn capture_stop(&self) -> anyhow::Result<CaptureStats> {
        let writer = self
            .shared
            .capture
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| anyhow!("no capture active on this session"))?;
        writer.finish()
    }

    fn shutdown(&self) {
        self.reader.abort();
        if let Some(writer) = self.shared.capture.lock().unwrap().take() {
            // Best effort: flush stats to disk; the file itself is already on disk.
            let _ = writer.finish();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

fn make_frame(ts_ms: u64, bytes: Vec<u8>) -> Frame {
    Frame { ts_ms, hex: hex::to_hex(&bytes), text: hex::to_text_lossy(&bytes) }
}

async fn reader_loop(mut read_half: ReadHalf<BoxedPort>, shared: Arc<Shared>) {
    let mut buf = [0u8; 4096];
    loop {
        match read_half.read(&mut buf).await {
            Ok(0) => {
                *shared.error.lock().unwrap() = Some("port closed (EOF)".to_string());
                shared.notify.notify_waiters();
                break;
            }
            Ok(n) => {
                let ts = now_ms();
                shared.last_rx_ms.store(ts, Ordering::Relaxed);
                shared.rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
                if let Err(e) = shared.capture_record("rx", ts, &buf[..n]) {
                    *shared.error.lock().unwrap() = Some(format!("capture write failed: {e}"));
                    shared.notify.notify_waiters();
                    break;
                }
                shared.buf.lock().unwrap().push(Chunk { ts_ms: ts, bytes: buf[..n].to_vec() });
                shared.notify.notify_waiters();
            }
            Err(e) => {
                *shared.error.lock().unwrap() = Some(e.to_string());
                shared.notify.notify_waiters();
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Session manager
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SessionManager {
    sessions: StdMutex<std::collections::HashMap<String, Arc<Session>>>,
    next_id: AtomicU64,
}

impl SessionManager {
    pub fn open(&self, port_name: &str, framing: Framing, port: BoxedPort) -> Arc<Session> {
        let id = format!("s{}", self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        let session = Session::spawn(id.clone(), port_name.to_string(), framing, port);
        self.sessions.lock().unwrap().insert(id, Arc::clone(&session));
        session
    }

    pub fn get(&self, id: &str) -> anyhow::Result<Arc<Session>> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(id).cloned().ok_or_else(|| {
            let mut open: Vec<String> = sessions.keys().cloned().collect();
            open.sort();
            anyhow!("no session {id:?}; open sessions: [{}]", open.join(", "))
        })
    }

    /// Every live session, snapshotted under a short lock and handed out as
    /// clones — callers inspect them lock-free. Sorted by id (numeric suffix
    /// order: s1, s2, … s10) for stable output.
    pub fn all(&self) -> Vec<Arc<Session>> {
        let mut out: Vec<Arc<Session>> =
            self.sessions.lock().unwrap().values().map(Arc::clone).collect();
        out.sort_by(|a, b| (a.id.len(), &a.id).cmp(&(b.id.len(), &b.id)));
        out
    }

    /// (port name, session id) for every live session — lets the port list say
    /// which ports are already held instead of leaving callers to hit EBUSY.
    pub fn open_ports(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .map(|s| (s.port_name.clone(), s.id.clone()))
            .collect();
        out.sort();
        out
    }

    pub fn close(&self, id: &str) -> anyhow::Result<()> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| anyhow!("no session {id:?} to close"))?;
        session.shutdown();
        Ok(())
    }
}
