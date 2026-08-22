//! Hardware-free integration tests: sessions over mock transports, and the
//! MCP tool dispatch end-to-end against a scaffolded workspace.

use openbaud::engine::audit::Audit;
use openbaud::engine::session::{Session, SessionManager};
use openbaud::engine::transport::{open_port, BoxedPort};
use openbaud::mcp::{tools, Ctx};
use openbaud::workspace::Workspace;
use openbaud_core::exec::{build_frame, parse_response};
use openbaud_core::format::{parse_command, Transport};
use openbaud_core::framing::{Framing, MatchRule};
use serde_json::{json, Map};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn echo_port() -> BoxedPort {
    open_port("mock:echo", &Transport::default()).await.expect("mock:echo always opens")
}

/// A port that answers one exact request with a canned reply.
fn scripted_port(expect: Vec<u8>, reply: Vec<u8>) -> BoxedPort {
    let (client, server) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        let (mut read, mut write) = tokio::io::split(server);
        let mut got = vec![0u8; expect.len()];
        if read.read_exact(&mut got).await.is_ok() {
            assert_eq!(got, expect, "device received unexpected request");
            write.write_all(&reply).await.expect("reply write");
        }
    });
    Box::new(client)
}

#[tokio::test]
async fn echo_session_idle_framing_round_trip() {
    let session = Session::spawn(
        "t1".into(),
        "mock:echo".into(),
        Framing::Idle { idle_ms: 20 },
        echo_port().await,
    );
    session.write_raw(b"hello").await.unwrap();
    let result = session.read_frames(1000, 32).await.unwrap();
    assert_eq!(result.frames.len(), 1);
    assert_eq!(result.frames[0].text, "hello");
    assert_eq!(result.dropped_bytes, 0);
}

#[tokio::test]
async fn request_timeout_is_loud() {
    // Echo port with a delimiter that never arrives.
    let session = Session::spawn(
        "t2".into(),
        "mock:echo".into(),
        Framing::Idle { idle_ms: 20 },
        echo_port().await,
    );
    let err = session
        .request(b"ping", MatchRule::Delimiter(b"\r\n".to_vec()), 100)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("does not satisfy"), "got: {err:#}");
}

const MODBUS_CMD: &str = r#"
schema: openbaud/command@v0
name: read_voltage
params:
  - { name: addr, type: u8, default: 1 }
frame:
  hex: "{addr} 04 00 00 00 01 {crc16_modbus}"
response:
  match: { length: 7 }
  validate: { checksum: crc16_modbus }
  parse:
    fields:
      voltage: { at: 3, type: u16be, scale: 0.1, unit: "V" }
"#;

#[tokio::test]
async fn scripted_modbus_command_end_to_end() {
    let cmd = parse_command(MODBUS_CMD, "read_voltage.yaml").unwrap();
    let tx = build_frame(&cmd, &Map::new()).unwrap();

    let mut reply = vec![0x01, 0x04, 0x02, 0x08, 0x9B]; // 0x089B = 2203 -> 220.3 V
    reply.extend(openbaud_core::checksum::ChecksumKind::Crc16Modbus.compute(&reply));

    let session = Session::spawn(
        "t3".into(),
        "scripted".into(),
        Framing::Idle { idle_ms: 20 },
        scripted_port(tx.clone(), reply),
    );
    let raw = session.request(&tx, MatchRule::Length(7), 1000).await.unwrap();
    let parsed = parse_response(&cmd, &raw).unwrap();
    assert_eq!(parsed, json!({ "voltage": 220.3 }));
}

fn scaffold_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let dev = dir.path().join("devices/echodev");
    std::fs::create_dir_all(dev.join("commands")).unwrap();
    std::fs::write(
        dev.join("profile.yaml"),
        "schema: openbaud/profile@v0\nname: echodev\nframing: { idle_ms: 20 }\n",
    )
    .unwrap();
    std::fs::write(
        dev.join("commands/ping.yaml"),
        r#"
schema: openbaud/command@v0
name: ping
params:
  - { name: x, type: u8, default: 66 }
frame: { hex: "AB {x} {sum8}" }
response:
  match: { length: 3 }
  parse: { fields: { y: { at: 1, type: u8 } } }
"#,
    )
    .unwrap();
    std::fs::write(
        dev.join("commands/wipe.yaml"),
        r#"
schema: openbaud/command@v0
name: wipe
description: pretend factory reset
risk: danger
frame: { hex: "DE AD" }
"#,
    )
    .unwrap();
    dir
}

fn ctx_for(dir: &tempfile::TempDir) -> Arc<Ctx> {
    Arc::new(Ctx {
        sessions: SessionManager::default(),
        workspace: Workspace::at(dir.path()),
        audit: Audit::new(dir.path()).unwrap(),
    })
}

#[tokio::test]
async fn tool_run_command_ephemeral_over_echo() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "ping", "port": "mock:echo" }),
        &ctx,
    )
    .await
    .unwrap();
    // Echo returns the frame verbatim: AB 42 ED, so y = 0x42 = 66.
    assert_eq!(result["parsed"]["y"], json!(66));
    assert_eq!(result["tx_hex"], json!("AB 42 ED"));

    let audit = std::fs::read_to_string(dir.path().join(".openbaud/audit.jsonl")).unwrap();
    assert!(audit.contains("\"run_command\""));
    assert!(audit.contains("\"ok\":true"));
}

#[tokio::test]
async fn tool_run_command_danger_requires_acknowledgement() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let err = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "wipe", "port": "mock:echo" }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("danger"), "got: {err:#}");

    // The denied attempt must leave an audit trace.
    let audit = std::fs::read_to_string(dir.path().join(".openbaud/audit.jsonl")).unwrap();
    assert!(audit.contains("\"denied\":true"), "audit: {audit}");
    assert!(audit.contains("\"ok\":false"), "audit: {audit}");

    // Acknowledged, it goes through (echo just swallows it; no response spec).
    let result = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "wipe", "port": "mock:echo", "acknowledge_risk": true }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(result["tx_hex"], json!("DE AD"));
}

#[tokio::test]
async fn tool_open_send_read_close_flow() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let opened = tools::call(
        "open",
        json!({ "port": "mock:echo", "framing": { "delimiter": "\n" } }),
        &ctx,
    )
    .await
    .unwrap();
    let sid = opened["session_id"].as_str().unwrap().to_string();

    tools::call("send", json!({ "session_id": sid, "text": "abc\ndef\n" }), &ctx)
        .await
        .unwrap();
    let read = tools::call("read", json!({ "session_id": sid, "timeout_ms": 1000 }), &ctx)
        .await
        .unwrap();
    let texts: Vec<&str> = read["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec!["abc", "def"]);

    tools::call("close", json!({ "session_id": sid }), &ctx).await.unwrap();
    let err = tools::call("read", json!({ "session_id": sid }), &ctx).await.unwrap_err();
    assert!(err.to_string().contains("no session"));
}

#[tokio::test]
async fn tool_capture_records_both_directions() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let opened = tools::call("open", json!({ "port": "mock:echo" }), &ctx).await.unwrap();
    let sid = opened["session_id"].as_str().unwrap().to_string();

    let started = tools::call("capture_start", json!({ "session_id": sid, "note": "t" }), &ctx)
        .await
        .unwrap();
    tools::call("send", json!({ "session_id": sid, "hex": "01 02" }), &ctx).await.unwrap();
    // Wait for the echo to come back so rx is captured too.
    tools::call("read", json!({ "session_id": sid, "timeout_ms": 1000 }), &ctx).await.unwrap();
    let stats = tools::call("capture_stop", json!({ "session_id": sid }), &ctx).await.unwrap();
    assert!(stats["bytes"].as_u64().unwrap() >= 4, "tx + rx bytes expected");
    assert!(stats["chunks"].as_u64().unwrap() >= 2, "tx + rx chunk records expected");
    assert!(stats.get("frames").is_none(), "stats field is named chunks, not frames");

    let content = std::fs::read_to_string(started["path"].as_str().unwrap()).unwrap();
    assert!(content.contains("\"tx\""));
    assert!(content.contains("\"rx\""));
    assert!(content.lines().next().unwrap().contains("\"obcap\":1"));
}

#[tokio::test]
async fn open_port_missing_device_fails_fast_without_retry() {
    // The busy-retry loop must only engage for busy-class errors; a missing
    // device errors immediately, with no retry note in the message.
    let started = std::time::Instant::now();
    let err = match open_port("/dev/tty.openbaud-test-does-not-exist", &Transport::default()).await
    {
        Ok(_) => panic!("opening a nonexistent port must fail"),
        Err(e) => e,
    };
    assert!(
        started.elapsed() < std::time::Duration::from_millis(500),
        "non-busy open errors must not be retried (took {:?})",
        started.elapsed()
    );
    let msg = format!("{err:#}");
    assert!(msg.contains("cannot open serial port"), "got: {msg}");
    assert!(!msg.contains("retries"), "no retry note expected, got: {msg}");
}

#[tokio::test]
async fn tool_open_rejects_invalid_numeric_params() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    for (args, needle) in [
        (json!({ "port": "mock:echo", "baud": -9600 }), "baud"),
        (json!({ "port": "mock:echo", "baud": "fast" }), "baud"),
        (json!({ "port": "mock:echo", "baud": 0 }), "baud"),
        (json!({ "port": "mock:echo", "data_bits": 9 }), "data_bits"),
        (json!({ "port": "mock:echo", "stop_bits": 3 }), "stop_bits"),
        (json!({ "port": "mock:echo", "parity": 7 }), "parity"),
    ] {
        let err = tools::call("open", args.clone(), &ctx).await.unwrap_err();
        assert!(
            err.to_string().contains(needle),
            "args {args} should fail mentioning {needle}, got: {err:#}"
        );
    }
}

#[tokio::test]
async fn broken_command_file_does_not_block_device() {
    let dir = scaffold_workspace();
    std::fs::write(
        dir.path().join("devices/echodev/commands/broken.yaml"),
        "schema: openbaud/command@v0\nname: broken\nframe: { hex: \"01\" }\nbogus_field: 1\n",
    )
    .unwrap();
    let ctx = ctx_for(&dir);

    // Other commands of the same device still execute, with a loud warning.
    let result = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "ping", "port": "mock:echo" }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(result["parsed"]["y"], json!(66));
    let warnings = result["warnings"].as_array().unwrap();
    assert!(warnings[0].as_str().unwrap().contains("broken.yaml"), "warnings: {warnings:?}");

    // A missing command names the broken file in its error.
    let err = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "nope", "port": "mock:echo" }),
        &ctx,
    )
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("broken.yaml") && msg.contains("bogus_field"), "got: {msg}");
}

#[tokio::test]
async fn tool_list_includes_mock_and_schemas() {
    let listed = tools::list();
    assert_eq!(listed.len(), 9);
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let ports = tools::call("list_ports", json!({}), &ctx).await.unwrap();
    let has_mock = ports["ports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["path"] == json!("mock:echo"));
    assert!(has_mock);
}
