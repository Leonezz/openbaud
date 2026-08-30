//! v0.1 engine slice, hardware-free: classified request outcomes, capture
//! replay, and selector → port resolution.

use openbaud::engine::session::Session;
use openbaud::engine::transport::{open_port, select_port, BoxedPort, PortInfo};
use openbaud_core::checksum::ChecksumKind;
use openbaud_core::exec::RawOutcome;
use openbaud_core::format::{ExceptionSpec, MatchSpec, SelectorSpec, Transport, WhenSpec};
use openbaud_core::framing::{Framing, MatchRule};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A port that answers one exact request with a canned reply, then stays
/// open — so a deliberately partial reply classifies as timeout, not as
/// port death.
fn scripted_port(expect: Vec<u8>, reply: Vec<u8>) -> BoxedPort {
    let (client, server) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        let (mut read, mut write) = tokio::io::split(server);
        let mut got = vec![0u8; expect.len()];
        if read.read_exact(&mut got).await.is_ok() {
            assert_eq!(got, expect, "device received unexpected request");
            write.write_all(&reply).await.expect("reply write");
            write.flush().await.ok();
            std::future::pending::<()>().await;
        }
    });
    Box::new(client)
}

/// A port that consumes everything and never replies (and never closes).
fn quiet_port() -> BoxedPort {
    let (client, server) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        let (mut read, _write) = tokio::io::split(server);
        let mut buf = [0u8; 1024];
        while matches!(read.read(&mut buf).await, Ok(n) if n > 0) {}
    });
    Box::new(client)
}

fn spawn_session(port: BoxedPort) -> Arc<Session> {
    Session::spawn("t".into(), "test".into(), Framing::Idle { idle_ms: 20 }, port)
}

/// Modbus exception recognition: `(byte[1] & FF) == 0x84`, 5-byte frame.
fn modbus_exception_spec() -> ExceptionSpec {
    ExceptionSpec {
        when: WhenSpec { at: 1, mask: None, equals: "84".to_string() },
        match_spec: MatchSpec { length: Some(5), delimiter: None, idle_ms: None },
        validate: None,
        parse: None,
    }
}

fn crc_framed(body: &[u8]) -> Vec<u8> {
    let mut frame = body.to_vec();
    frame.extend(ChecksumKind::Crc16Modbus.compute(body));
    frame
}

// ---------------------------------------------------------------------------
// request_classified
// ---------------------------------------------------------------------------

const TX: &[u8] = &[0x01, 0x04, 0x00, 0x00, 0x00, 0x01, 0x31, 0xCA];

#[tokio::test]
async fn classified_normal_frame() {
    let reply = crc_framed(&[0x01, 0x04, 0x02, 0x08, 0x9B]); // 7 bytes
    let session = spawn_session(scripted_port(TX.to_vec(), reply.clone()));
    let out = session
        .request_classified(TX, MatchRule::Length(7), Some(&modbus_exception_spec()), 1000, None)
        .await
        .unwrap();
    assert_eq!(out, RawOutcome::Frame { bytes: reply, is_exception: false });
}

#[tokio::test]
async fn classified_exception_frame() {
    let reply = crc_framed(&[0x01, 0x84, 0x01]); // FC|0x80 exception, 5 bytes
    let session = spawn_session(scripted_port(TX.to_vec(), reply.clone()));
    let out = session
        .request_classified(TX, MatchRule::Length(7), Some(&modbus_exception_spec()), 1000, None)
        .await
        .unwrap();
    assert_eq!(out, RawOutcome::Frame { bytes: reply, is_exception: true });
}

#[tokio::test]
async fn classified_silence_returns_early_on_first_byte_window() {
    let session = spawn_session(quiet_port());
    let started = Instant::now();
    let out = session
        .request_classified(TX, MatchRule::Length(7), None, 3000, Some(100))
        .await
        .unwrap();
    assert_eq!(out, RawOutcome::Silence);
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "first_byte_ms must classify silence well before timeout_ms (took {:?})",
        started.elapsed()
    );
}

#[tokio::test]
async fn classified_timeout_carries_partial_bytes() {
    let partial = vec![0x01, 0x04, 0x02]; // 3 of the 7 expected bytes
    let session = spawn_session(scripted_port(TX.to_vec(), partial.clone()));
    let out = session
        .request_classified(TX, MatchRule::Length(7), Some(&modbus_exception_spec()), 300, None)
        .await
        .unwrap();
    assert_eq!(out, RawOutcome::Timeout { partial });
}

#[tokio::test]
async fn classified_short_normal_frame_wins_while_exception_undecided() {
    // Normal frame shorter than when.at + 1: the main rule must still fire.
    let session = spawn_session(scripted_port(TX.to_vec(), vec![0x06]));
    let out = session
        .request_classified(TX, MatchRule::Length(1), Some(&modbus_exception_spec()), 1000, None)
        .await
        .unwrap();
    assert_eq!(out, RawOutcome::Frame { bytes: vec![0x06], is_exception: false });
}

// ---------------------------------------------------------------------------
// Capture replay
// ---------------------------------------------------------------------------

/// Drive mock:echo with two request/response exchanges under an active
/// capture; returns the .obcap path.
async fn record_echo_capture(dir: &tempfile::TempDir) -> String {
    let port = open_port("mock:echo", &Transport::default(), None).await.unwrap();
    let session = spawn_session(port);
    let path = session
        .capture_start(&dir.path().join("echo.obcap"), Some("replay test"))
        .unwrap();
    for payload in [&[0x01u8, 0x02][..], &[0x03, 0x04][..]] {
        session.write_raw(payload).await.unwrap();
        let frames = session.read_frames(1000, 8).await.unwrap().frames;
        assert_eq!(frames.len(), 1, "echo must return the payload");
    }
    session.capture_stop().unwrap();
    path
}

#[tokio::test]
async fn replay_reproduces_recorded_exchange() {
    let dir = tempfile::tempdir().unwrap();
    let path = record_echo_capture(&dir).await;

    let port = open_port(&format!("replay:{path}"), &Transport::default(), None).await.unwrap();
    let session = spawn_session(port);
    for (payload, hex) in [(&[0x01u8, 0x02][..], "01 02"), (&[0x03, 0x04][..], "03 04")] {
        session.write_raw(payload).await.unwrap();
        let frames = session.read_frames(1000, 8).await.unwrap().frames;
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].hex, hex);
    }
}

#[tokio::test]
async fn replay_rejects_diverging_tx() {
    let dir = tempfile::tempdir().unwrap();
    let path = record_echo_capture(&dir).await;

    let port = open_port(&format!("replay:{path}"), &Transport::default(), None).await.unwrap();
    let session = spawn_session(port);
    session.write_raw(&[0xFF]).await.unwrap(); // capture expects 01 02
    let err = session.read_frames(1000, 8).await.unwrap_err();
    assert!(
        err.to_string().contains("TX diverged from capture: expected"),
        "got: {err:#}"
    );
}

#[tokio::test]
async fn replay_errors_when_capture_is_exhausted() {
    let dir = tempfile::tempdir().unwrap();
    let path = record_echo_capture(&dir).await;

    let port = open_port(&format!("replay:{path}"), &Transport::default(), None).await.unwrap();
    let session = spawn_session(port);
    for payload in [&[0x01u8, 0x02][..], &[0x03, 0x04][..]] {
        session.write_raw(payload).await.unwrap();
        session.read_frames(1000, 8).await.unwrap();
    }
    session.write_raw(&[0x05]).await.unwrap(); // nothing left in the capture
    let err = session.read_frames(1000, 8).await.unwrap_err();
    assert!(err.to_string().contains("capture exhausted"), "got: {err:#}");
}

/// `unwrap_err` needs `Debug` on the Ok type, which `BoxedPort` lacks.
fn expect_open_error(result: anyhow::Result<BoxedPort>) -> anyhow::Error {
    match result {
        Ok(_) => panic!("open must fail"),
        Err(e) => e,
    }
}

#[tokio::test]
async fn replay_open_fails_loudly_on_bad_files() {
    let err =
        expect_open_error(open_port("replay:/nonexistent/no.obcap", &Transport::default(), None).await);
    assert!(err.to_string().contains("cannot open capture file"), "got: {err:#}");

    let dir = tempfile::tempdir().unwrap();
    let bogus = dir.path().join("not-a-capture.obcap");
    std::fs::write(&bogus, "{\"foo\": 1}\n").unwrap();
    let err = expect_open_error(
        open_port(&format!("replay:{}", bogus.display()), &Transport::default(), None).await,
    );
    assert!(err.to_string().contains("obcap"), "got: {err:#}");
}

// ---------------------------------------------------------------------------
// Selector matching (pure core of resolve_selector)
// ---------------------------------------------------------------------------

fn usb(path: &str, vid: &str, pid: &str, serial: Option<&str>, product: Option<&str>) -> PortInfo {
    PortInfo {
        path: path.to_string(),
        kind: "usb",
        vid: Some(vid.to_string()),
        pid: Some(pid.to_string()),
        manufacturer: None,
        product: product.map(str::to_string),
        serial_number: serial.map(str::to_string),
    }
}

fn mock_entry() -> PortInfo {
    PortInfo {
        path: "mock:echo".to_string(),
        kind: "mock",
        vid: None,
        pid: None,
        manufacturer: None,
        product: Some("loopback echo".to_string()),
        serial_number: None,
    }
}

fn selector(vid: Option<&str>, pid: Option<&str>) -> SelectorSpec {
    SelectorSpec {
        vid: vid.map(str::to_string),
        pid: pid.map(str::to_string),
        serial_number: None,
        product: None,
    }
}

#[test]
fn select_port_unique_hit_is_case_insensitive() {
    let ports = vec![
        usb("/dev/cu.usbserial-110", "1A86", "55D3", None, Some("USB Single Serial")),
        usb("/dev/cu.usbmodem-2", "10C4", "EA60", None, Some("CP2102")),
        mock_entry(),
    ];
    let path = select_port(&ports, &selector(Some("1a86"), Some("55d3")), "pzem004t").unwrap();
    assert_eq!(path, "/dev/cu.usbserial-110");
}

#[test]
fn select_port_prefers_cu_over_tty_for_the_same_physical_port() {
    let ports = vec![
        usb("/dev/tty.usbserial-110", "1A86", "55D3", None, None),
        usb("/dev/cu.usbserial-110", "1A86", "55D3", None, None),
    ];
    let path = select_port(&ports, &selector(Some("1A86"), None), "pzem004t").unwrap();
    assert_eq!(path, "/dev/cu.usbserial-110");
}

#[test]
fn select_port_zero_matches_lists_criteria_and_candidates() {
    let ports = vec![
        usb("/dev/cu.usbmodem-2", "10C4", "EA60", None, Some("CP2102")),
        mock_entry(),
    ];
    let err = select_port(&ports, &selector(Some("1A86"), Some("55D3")), "pzem004t").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("no port matches"), "got: {msg}");
    assert!(msg.contains("pzem004t"), "got: {msg}");
    assert!(msg.contains("vid=1A86") && msg.contains("pid=55D3"), "got: {msg}");
    assert!(msg.contains("/dev/cu.usbmodem-2"), "got: {msg}");
    assert!(!msg.contains("mock:echo"), "mock entries are not candidates: {msg}");
}

#[test]
fn select_port_multiple_matches_demands_explicit_port() {
    let ports = vec![
        usb("/dev/cu.usbserial-110", "1A86", "55D3", None, None),
        usb("/dev/cu.usbserial-230", "1A86", "55D3", None, None),
    ];
    let err = select_port(&ports, &selector(Some("1A86"), None), "pzem004t").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("matches 2 ports"), "got: {msg}");
    assert!(
        msg.contains("/dev/cu.usbserial-110") && msg.contains("/dev/cu.usbserial-230"),
        "got: {msg}"
    );
    assert!(msg.contains("explicitly"), "got: {msg}");
}

#[test]
fn select_port_never_matches_mock_entries() {
    // A selector that would substring-match the mock's product must not pick it.
    let ports = vec![mock_entry()];
    let sel = SelectorSpec {
        vid: None,
        pid: None,
        serial_number: None,
        product: Some("loopback".to_string()),
    };
    assert!(select_port(&ports, &sel, "dev").is_err());
}
