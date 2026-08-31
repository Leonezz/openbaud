//! Hardware-free integration tests: sessions over mock transports, and the
//! MCP tool dispatch end-to-end against a scaffolded workspace.

use openbaud::engine::audit::Audit;
use openbaud::engine::session::{Session, SessionManager};
use openbaud::engine::stream::{
    DEFAULT_POLL_INLINE_BYTES, MAX_POLL_INLINE_BYTES, MAX_RETAINED_FRAMES,
    MAX_SUBSCRIPTIONS_PER_SESSION, SUBSCRIPTION_IDLE_TTL_MS,
};
use openbaud::engine::transport::{open_port, BoxedPort};
use openbaud::mcp::{tools, Ctx};
use openbaud::workspace::Workspace;
use openbaud_core::exec::{build_frame, parse_response};
use openbaud_core::format::{parse_command, Transport};
use openbaud_core::framing::{Deframer, Framing, MatchRule};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn echo_port() -> BoxedPort {
    open_port("mock:echo", &Transport::default(), None).await.expect("mock:echo always opens")
}

/// A port that answers one exact request with a canned reply, then stays
/// open — so classification is decided by the match rules, not port death.
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
        client_info: Default::default(),
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
    // The result names the port it actually used (viewer replay watermark).
    assert_eq!(result["port"], json!("mock:echo"));

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

    // Both results carry the workspace-relative path (directly composable with
    // the data tools) plus the absolute one.
    for result in [&started, &stats] {
        let rel = result["path"].as_str().unwrap();
        assert!(rel.starts_with("captures/"), "workspace-relative path expected, got: {rel}");
        let abs = result["abs_path"].as_str().unwrap();
        assert!(std::path::Path::new(abs).is_absolute(), "got: {abs}");
    }
    assert_eq!(started["path"], stats["path"]);

    let content = std::fs::read_to_string(started["abs_path"].as_str().unwrap()).unwrap();
    assert!(content.contains("\"tx\""));
    assert!(content.contains("\"rx\""));
    assert!(content.lines().next().unwrap().contains("\"obcap\":1"));
}

#[tokio::test]
async fn capture_paths_compose_with_the_data_tools() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let opened = tools::call("open", json!({ "port": "mock:echo" }), &ctx).await.unwrap();
    let sid = opened["session_id"].as_str().unwrap().to_string();
    let started = tools::call("capture_start", json!({ "session_id": sid }), &ctx).await.unwrap();
    tools::call("send", json!({ "session_id": sid, "hex": "01 02" }), &ctx).await.unwrap();
    tools::call("read", json!({ "session_id": sid, "timeout_ms": 1000 }), &ctx).await.unwrap();
    let stopped = tools::call("capture_stop", json!({ "session_id": sid }), &ctx).await.unwrap();

    // The returned relative path feeds capture_frames and session_timeline verbatim.
    for path in [started["path"].as_str().unwrap(), stopped["path"].as_str().unwrap()] {
        let frames = tools::call(
            "capture_frames",
            json!({ "path": path, "framing": { "idle_ms": 30 } }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(frames["total_in_window"].as_u64().unwrap() >= 1, "got: {frames}");
        let timeline =
            tools::call("session_timeline", json!({ "path": path }), &ctx).await.unwrap();
        assert_eq!(timeline["view"]["kind"], json!("timeline"));
    }

    // The absolute path is accepted too — it canonicalizes into captures/.
    let abs = started["abs_path"].as_str().unwrap();
    let frames = tools::call(
        "capture_frames",
        json!({ "path": abs, "framing": { "idle_ms": 30 } }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(frames["source"]["path"], started["path"], "absolute input normalizes to relative");

    // An absolute path outside captures/ is still refused loudly.
    let outside = dir.path().join("outside.obcap");
    std::fs::write(&outside, "{\"obcap\":1}\n").unwrap();
    let err = tools::call(
        "session_timeline",
        json!({ "path": outside.display().to_string() }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("captures"), "got: {err:#}");
}

#[tokio::test]
async fn open_port_missing_device_fails_fast_without_retry() {
    // The busy-retry loop must only engage for busy-class errors; a missing
    // device errors immediately, with no retry note in the message.
    let started = std::time::Instant::now();
    let err = match open_port("/dev/tty.openbaud-test-does-not-exist", &Transport::default(), None).await
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
    assert_eq!(listed.len(), 18);
    for tool in &listed {
        let annotations = tool["annotations"].as_object().unwrap_or_else(|| {
            panic!("tool {} is missing MCP behavior annotations", tool["name"])
        });
        for hint in ["readOnlyHint", "openWorldHint", "destructiveHint"] {
            assert!(
                annotations.get(hint).and_then(serde_json::Value::as_bool).is_some(),
                "tool {} is missing boolean annotation {hint}",
                tool["name"]
            );
        }
    }
    for name in ["send", "request", "run_command", "run_workflow"] {
        let tool = listed.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["destructiveHint"], json!(true), "tool {name}");
    }
    for name in [
        "list_ports",
        "read",
        "schema",
        "session_timeline",
        "capture_frames",
        "diagnose_frame",
        "session_stats",
        "stream_poll",
    ] {
        let tool = listed.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], json!(true), "tool {name}");
    }
    // The capture/audit analysis tools are local: they never touch hardware.
    for name in ["session_timeline", "capture_frames", "diagnose_frame"] {
        let tool = listed.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["openWorldHint"], json!(false), "tool {name}");
    }
    for name in [
        "list_ports",
        "open",
        "close",
        "read",
        "send",
        "request",
        "run_command",
        "run_workflow",
        "session_stats",
        "stream_poll",
    ] {
        let tool = listed.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["openWorldHint"], json!(true), "tool {name}");
    }
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

// ---------------------------------------------------------------------------
// v0.1 surface: classified outcomes, workflows, selector and replay wiring
// ---------------------------------------------------------------------------

const MODBUS_TX: &[u8] = &[0x01, 0x04, 0x00, 0x00, 0x00, 0x01, 0x31, 0xCA];

fn crc_framed(body: &[u8]) -> Vec<u8> {
    let mut frame = body.to_vec();
    frame.extend(openbaud_core::checksum::ChecksumKind::Crc16Modbus.compute(body));
    frame
}

/// Command with exception recognition; `expect` is spliced in by the caller.
fn modbus_cmd_yaml(name: &str, expect: &str) -> String {
    format!(
        r#"
schema: openbaud/command@v0
name: {name}
frame: {{ hex: "01 04 00 00 00 01 {{crc16_modbus}}" }}
response:
  match: {{ length: 7 }}
  timeout_ms: 1000
{expect}  validate: {{ checksum: crc16_modbus }}
  parse:
    fields:
      voltage: {{ at: 3, type: u16be, scale: 0.1, unit: "V" }}
  exception:
    when: {{ at: 1, equals: "84" }}
    match: {{ length: 5 }}
    validate: {{ checksum: crc16_modbus }}
    parse:
      fields:
        function: {{ at: 1, type: u8 }}
        exception_code: {{ at: 2, type: u8 }}
"#
    )
}

#[tokio::test]
async fn tool_run_command_classifies_modbus_exception() {
    let dir = scaffold_workspace();
    let cmds = dir.path().join("devices/echodev/commands");
    std::fs::write(cmds.join("read_v.yaml"), modbus_cmd_yaml("read_v", "")).unwrap();
    std::fs::write(
        cmds.join("illegal.yaml"),
        modbus_cmd_yaml("illegal", "  expect: exception\n"),
    )
    .unwrap();
    let ctx = ctx_for(&dir);
    let exception_reply = crc_framed(&[0x01, 0x84, 0x01]);

    // Default expect (normal) + exception frame on the wire: classified, but
    // the expectation is unmet — the error embeds the full result JSON.
    let session = ctx.sessions.open(
        "scripted",
        Framing::Idle { idle_ms: 20 },
        scripted_port(MODBUS_TX.to_vec(), exception_reply.clone()),
    );
    let err = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "read_v", "session_id": session.id }),
        &ctx,
    )
    .await
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("expectation not met"), "got: {msg}");
    assert!(msg.contains("\"outcome\": \"exception\""), "got: {msg}");
    assert!(msg.contains("\"exception_code\": 1"), "got: {msg}");

    // Same wire response, but the command declares expect: exception.
    let session = ctx.sessions.open(
        "scripted",
        Framing::Idle { idle_ms: 20 },
        scripted_port(MODBUS_TX.to_vec(), exception_reply),
    );
    let result = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "illegal", "session_id": session.id }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(result["outcome"], json!("exception"));
    assert_eq!(result["expect"], json!("exception"));
    assert_eq!(result["expect_met"], json!(true));
    assert_eq!(result["exception"]["exception_code"], json!(1));
    assert_eq!(result["exception"]["function"], json!(0x84));

    let audit = std::fs::read_to_string(dir.path().join(".openbaud/audit.jsonl")).unwrap();
    assert!(audit.contains("\"outcome\":\"exception\""), "audit: {audit}");
}

#[tokio::test]
async fn tool_run_workflow_failure_skips_steps_but_runs_finally() {
    let dir = scaffold_workspace();
    let dev = dir.path().join("devices/echodev");
    // Echo always answers, so expect: silence must fail.
    std::fs::write(
        dev.join("commands/must_fail.yaml"),
        r#"
schema: openbaud/command@v0
name: must_fail
frame: { hex: "AA" }
response:
  expect: silence
  timeout_ms: 200
"#,
    )
    .unwrap();
    std::fs::create_dir_all(dev.join("workflows")).unwrap();
    std::fs::write(
        dev.join("workflows/check.yaml"),
        r#"
schema: openbaud/workflow@v0
name: check
steps:
  - command: must_fail
  - command: ping
finally:
  - command: ping
"#,
    )
    .unwrap();
    let ctx = ctx_for(&dir);

    let err = tools::call(
        "run_workflow",
        json!({ "device": "echodev", "workflow": "check", "port": "mock:echo" }),
        &ctx,
    )
    .await
    .unwrap_err();
    let msg = format!("{err:#}");
    let (_, embedded) = msg.split_once("Full result:\n").expect("error embeds the result JSON");
    let result: serde_json::Value = serde_json::from_str(embedded.trim()).unwrap();

    assert_eq!(result["ok"], json!(false));
    assert_eq!(result["steps"].as_array().unwrap().len(), 1, "second step must not run");
    assert_eq!(result["steps"][0]["command"], json!("must_fail"));
    assert_eq!(result["steps"][0]["outcome"], json!("timeout"));
    assert_eq!(result["steps"][0]["expect_met"], json!(false));
    assert_eq!(result["skipped"], json!(["ping"]));
    // finally still ran, and succeeded.
    assert_eq!(result["finally"][0]["command"], json!("ping"));
    assert_eq!(result["finally"][0]["ok"], json!(true));

    let audit = std::fs::read_to_string(dir.path().join(".openbaud/audit.jsonl")).unwrap();
    let finally_line = audit
        .lines()
        .find(|l| l.contains("run_workflow.step") && l.contains("\"finally\""))
        .expect("finally step must be audited");
    assert!(finally_line.contains("\"ok\":true"), "got: {finally_line}");
    let total_line = audit
        .lines()
        .find(|l| l.contains("\"tool\":\"run_workflow\""))
        .expect("workflow-level audit entry");
    assert!(total_line.contains("\"ok\":false"), "got: {total_line}");
}

#[tokio::test]
async fn workflow_referencing_missing_command_is_broken() {
    let dir = scaffold_workspace();
    let wf_dir = dir.path().join("devices/echodev/workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("bad.yaml"),
        "schema: openbaud/workflow@v0\nname: badwf\nsteps:\n  - command: nope\n",
    )
    .unwrap();
    let ctx = ctx_for(&dir);

    let err = tools::call(
        "run_workflow",
        json!({ "device": "echodev", "workflow": "badwf", "port": "mock:echo" }),
        &ctx,
    )
    .await
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("bad.yaml"), "got: {msg}");
    assert!(msg.contains("nope"), "got: {msg}");
}

#[tokio::test]
async fn selector_with_zero_matches_reports_candidates() {
    let dir = scaffold_workspace();
    let dev = dir.path().join("devices/seldev");
    std::fs::create_dir_all(dev.join("commands")).unwrap();
    std::fs::write(
        dev.join("profile.yaml"),
        "schema: openbaud/profile@v0\nname: seldev\nselector: { serial_number: \"OPENBAUD-TEST-NO-SUCH-SERIAL\" }\n",
    )
    .unwrap();
    std::fs::write(
        dev.join("commands/noop.yaml"),
        "schema: openbaud/command@v0\nname: noop\nframe: { hex: \"00\" }\n",
    )
    .unwrap();
    let ctx = ctx_for(&dir);

    // No port and no session: the selector runs and finds nothing — loud
    // error describing the criteria, never a silent pick.
    let err = tools::call(
        "run_command",
        json!({ "device": "seldev", "command": "noop" }),
        &ctx,
    )
    .await
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("no port matches"), "got: {msg}");
    assert!(msg.contains("OPENBAUD-TEST-NO-SUCH-SERIAL"), "got: {msg}");
}

#[tokio::test]
async fn replay_relative_path_resolves_against_workspace() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    // Record: run ping over mock:echo with a capture active.
    let opened = tools::call("open", json!({ "port": "mock:echo", "device": "echodev" }), &ctx)
        .await
        .unwrap();
    let sid = opened["session_id"].as_str().unwrap().to_string();
    let started = tools::call("capture_start", json!({ "session_id": sid }), &ctx).await.unwrap();
    let live = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "ping", "session_id": sid }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(live["outcome"], json!("normal"));
    tools::call("capture_stop", json!({ "session_id": sid }), &ctx).await.unwrap();
    tools::call("close", json!({ "session_id": sid }), &ctx).await.unwrap();

    // Replay via the workspace-relative capture path capture_start returned.
    let rel = started["path"].as_str().unwrap();
    assert!(rel.starts_with("captures/"), "got: {rel}");
    let replayed = tools::call(
        "run_command",
        json!({
            "device": "echodev",
            "command": "ping",
            "port": format!("replay:{rel}"),
        }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(replayed["outcome"], json!("normal"));
    assert_eq!(replayed["expect_met"], json!(true));
    assert_eq!(replayed["parsed"]["y"], json!(66));
    assert_eq!(replayed["parsed"], live["parsed"], "replay must reproduce the live result");
    // The port field discloses the replay transport so a viewer can watermark it.
    let replay_port = replayed["port"].as_str().unwrap();
    assert!(replay_port.starts_with("replay:"), "got: {replay_port}");
    assert_eq!(live["port"], json!("mock:echo"));
}

#[tokio::test]
async fn replay_keeps_idle_framed_boundaries_deterministic() {
    // Recorded gaps of 200 ms with idle_ms: 100 framing: the offline deframe
    // yields three rx frames. Replay compresses long gaps, but a gap the
    // session's idle framing splits on must stay clearly above the threshold —
    // compressing it to exactly idle_ms makes live framing nondeterministic
    // under the 10 ms poll.
    let dir = scaffold_workspace();
    let captures = dir.path().join("captures");
    std::fs::create_dir_all(&captures).unwrap();
    std::fs::write(
        captures.join("gap.obcap"),
        concat!(
            r#"{"obcap":1,"session":"sg","port":"mock:echo","note":null,"started_ms":1000}"#,
            "\n",
            r#"{"ts_ms":1000,"dir":"tx","hex":"AA"}"#,
            "\n",
            r#"{"ts_ms":1010,"dir":"rx","hex":"01"}"#,
            "\n",
            r#"{"ts_ms":1210,"dir":"rx","hex":"02"}"#,
            "\n",
            r#"{"ts_ms":1410,"dir":"rx","hex":"03"}"#,
            "\n",
        ),
    )
    .unwrap();
    let ctx = ctx_for(&dir);

    let offline = tools::call(
        "capture_frames",
        json!({ "path": "captures/gap.obcap", "framing": { "idle_ms": 100 } }),
        &ctx,
    )
    .await
    .unwrap();
    let offline_rx: Vec<&str> = offline["frames"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["dir"] == json!("rx"))
        .map(|f| f["hex"].as_str().unwrap())
        .collect();
    assert_eq!(offline_rx, vec!["01", "02", "03"], "got: {offline}");

    let opened = tools::call(
        "open",
        json!({ "port": "replay:captures/gap.obcap", "framing": { "idle_ms": 100 } }),
        &ctx,
    )
    .await
    .unwrap();
    let sid = opened["session_id"].as_str().unwrap().to_string();
    tools::call("send", json!({ "session_id": sid, "hex": "AA" }), &ctx).await.unwrap();

    let mut live: Vec<String> = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(2500);
    while live.len() < offline_rx.len() && tokio::time::Instant::now() < deadline {
        let read = tools::call("read", json!({ "session_id": sid, "timeout_ms": 200 }), &ctx)
            .await
            .unwrap();
        for f in read["frames"].as_array().unwrap() {
            live.push(f["hex"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(
        live, offline_rx,
        "a live replay read must reproduce the offline frame boundaries"
    );
    tools::call("close", json!({ "session_id": sid }), &ctx).await.unwrap();
}

// ---------------------------------------------------------------------------
// Output shaping (spill-to-disk summaries) and schema surfacing
// ---------------------------------------------------------------------------

/// 400-element u8 array response: 2 header bytes + 400 payload bytes.
const BIG_ARRAY_CMD: &str = r#"
schema: openbaud/command@v0
name: big_read
frame: { hex: "01" }
response:
  match: { length: 402 }
  timeout_ms: 2000
  parse:
    fields:
      samples: { at: 2, type: u8, count: 400 }
"#;

fn big_array_reply() -> Vec<u8> {
    let mut reply = vec![0x00, 0x00];
    reply.extend((0..400u32).map(|i| (i % 251) as u8));
    reply
}

#[tokio::test]
async fn oversized_result_spills_to_disk_with_summary() {
    let dir = scaffold_workspace();
    std::fs::write(dir.path().join("devices/echodev/commands/big_read.yaml"), BIG_ARRAY_CMD)
        .unwrap();
    let ctx = ctx_for(&dir);

    let session = ctx.sessions.open(
        "scripted",
        Framing::Idle { idle_ms: 20 },
        scripted_port(vec![0x01], big_array_reply()),
    );
    let result = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "big_read", "session_id": session.id }),
        &ctx,
    )
    .await
    .unwrap();

    // Inline view: array head + explicit truncation marker, strings cut with
    // a byte-count note, scalars untouched.
    let samples = result["parsed"]["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 9, "8 head elements + truncation marker");
    assert_eq!(samples[0], json!(0));
    assert_eq!(samples[8], json!({ "truncated": 392 }));
    let raw_hex = result["raw_hex"].as_str().unwrap();
    assert!(raw_hex.contains("…("), "long strings carry a truncation note, got: {raw_hex}");
    assert_eq!(result["outcome"], json!("normal"));

    // The full-result file holds every element.
    let rel = result["full_result"].as_str().unwrap();
    assert!(rel.starts_with(".openbaud/out/"), "workspace-relative path, got: {rel}");
    let full: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(rel)).unwrap()).unwrap();
    let full_samples = full["parsed"]["samples"].as_array().unwrap();
    assert_eq!(full_samples.len(), 400);
    assert_eq!(full_samples[399], json!(399 % 251));
    assert!(full.get("full_result").is_none(), "the spilled file is the untouched result");

    // Raising max_inline_bytes forces the same call fully inline.
    let session = ctx.sessions.open(
        "scripted",
        Framing::Idle { idle_ms: 20 },
        scripted_port(vec![0x01], big_array_reply()),
    );
    let inline = tools::call(
        "run_command",
        json!({
            "device": "echodev",
            "command": "big_read",
            "session_id": session.id,
            "max_inline_bytes": 1_000_000,
        }),
        &ctx,
    )
    .await
    .unwrap();
    assert!(inline.get("full_result").is_none(), "inline result must not spill");
    assert_eq!(inline["parsed"]["samples"].as_array().unwrap().len(), 400);
    assert_eq!(inline["parsed"]["samples"], full["parsed"]["samples"]);
}

#[tokio::test]
async fn schema_tool_returns_schema_and_parsable_example() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = tools::call("schema", json!({ "kind": "command" }), &ctx).await.unwrap();
    assert_eq!(result["kind"], json!("command"));
    assert_eq!(result["schema"]["$id"], json!("openbaud/command@v0"));
    assert_eq!(result["schema"]["additionalProperties"], json!(false));

    // example: true returns YAML that the real parser accepts (the command
    // example is two `---`-separated documents: binary, then text).
    let result = tools::call("schema", json!({ "kind": "command", "example": true }), &ctx)
        .await
        .unwrap();
    assert!(result.get("schema").is_none());
    let yaml = result["example"].as_str().unwrap();
    let docs: Vec<&str> = yaml.split("\n---\n").collect();
    assert_eq!(docs.len(), 2, "command example holds two documents");
    for doc in docs {
        parse_command(doc, "example.yaml").unwrap();
    }

    let err = tools::call("schema", json!({ "kind": "bogus" }), &ctx).await.unwrap_err();
    assert!(err.to_string().contains("bogus"), "got: {err:#}");
}

#[test]
fn cli_schema_example_prints_yaml() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_openbaud"))
        .args(["schema", "command", "--example"])
        .output()
        .expect("openbaud binary runs");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.trim().is_empty());
    assert!(stdout.contains("openbaud/command@v0"), "got: {stdout}");
}

// ---- MCP protocol surface (JSON-RPC layer, for Apps hosts) ----

#[tokio::test]
async fn rpc_tool_call_carries_structured_content() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = openbaud::mcp::handle("tools/call", json!({ "name": "list_ports", "arguments": {} }), &ctx)
        .await
        .expect("list_ports succeeds");

    let structured = result.get("structuredContent").expect("structuredContent present");
    assert!(structured["ports"].is_array(), "structured: {structured}");
    // Widget and model must see the same data: text block parses to structuredContent.
    let text = result["content"][0]["text"].as_str().expect("text block");
    let from_text: Value = serde_json::from_str(text).expect("text block is JSON");
    assert_eq!(&from_text, structured);
}

#[tokio::test]
async fn rpc_tool_error_stays_text_only() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = openbaud::mcp::handle(
        "tools/call",
        json!({ "name": "run_command", "arguments": { "device": "echodev", "command": "wipe", "port": "mock:echo" } }),
        &ctx,
    )
    .await
    .expect("tool errors are Ok envelopes with isError");

    assert_eq!(result["isError"], json!(true));
    assert!(result.get("structuredContent").is_none(), "error path must stay byte-identical for non-Apps hosts");
    let text = result["content"][0]["text"].as_str().expect("text block");
    assert!(text.contains("danger"), "got: {text}");
}

#[tokio::test]
async fn rpc_initialize_records_client_info() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = openbaud::mcp::handle(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "clientInfo": { "name": "claude-desktop", "version": "1.0.0" }
        }),
        &ctx,
    )
    .await
    .expect("initialize succeeds");

    assert!(result["capabilities"]["tools"].is_object());
    let recorded = ctx.client_info.get().expect("clientInfo recorded for host routing");
    assert_eq!(recorded["name"], json!("claude-desktop"));
}

#[tokio::test]
async fn rpc_initialize_declares_resources_capability() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = openbaud::mcp::handle("initialize", json!({}), &ctx).await.unwrap();
    assert!(result["capabilities"]["resources"].is_object(), "got: {result}");
}

#[tokio::test]
async fn rpc_resources_list_serves_ui_templates() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = openbaud::mcp::handle("resources/list", json!({}), &ctx).await.unwrap();
    let resources = result["resources"].as_array().expect("resources array");
    assert_eq!(resources.len(), 2);
    for r in resources {
        let uri = r["uri"].as_str().expect("uri");
        assert!(uri.starts_with("ui://openbaud/"), "got: {uri}");
        assert_eq!(r["mimeType"], json!("text/html;profile=mcp-app"));
        assert!(r["name"].is_string());
    }
}

#[tokio::test]
async fn rpc_resources_templates_list_declares_the_result_template() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = openbaud::mcp::handle("resources/templates/list", json!({}), &ctx).await.unwrap();
    let templates = result["resourceTemplates"].as_array().expect("resourceTemplates array");
    assert_eq!(templates.len(), 1, "got: {templates:?}");
    let t = &templates[0];
    assert_eq!(t["uriTemplate"], json!("openbaud://result/{name}"));
    assert_eq!(t["mimeType"], json!("application/json"));
    assert!(t["name"].is_string() && t["description"].is_string(), "got: {t}");
}

#[tokio::test]
async fn rpc_resources_read_returns_embedded_html() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = openbaud::mcp::handle(
        "resources/read",
        json!({ "uri": "ui://openbaud/viewer.html" }),
        &ctx,
    )
    .await
    .unwrap();

    let content = &result["contents"][0];
    assert_eq!(content["uri"], json!("ui://openbaud/viewer.html"));
    assert_eq!(content["mimeType"], json!("text/html;profile=mcp-app"));
    let html = content["text"].as_str().expect("inline html");
    assert!(html.len() > 10_000, "embedded widget looks too small: {} bytes", html.len());
    assert!(html.to_lowercase().contains("<!doctype html"), "not an html document");
}

#[tokio::test]
async fn rpc_resources_read_unknown_uri_is_loud() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let err = openbaud::mcp::handle(
        "resources/read",
        json!({ "uri": "ui://openbaud/nope.html" }),
        &ctx,
    )
    .await
    .expect_err("unknown uri must be an RPC error, not empty contents");
    assert_eq!(err.code, -32002);
    assert!(err.message.contains("nope.html"), "got: {}", err.message);
}

#[tokio::test]
async fn results_report_a_checksum_only_when_one_was_verified() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    // echodev/ping declares no `validate`, so nothing about it was checked —
    // the result must not imply otherwise.
    let unchecked = tools::call(
        "run_command",
        json!({ "device": "echodev", "command": "ping", "port": "mock:echo" }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(unchecked["outcome"], json!("normal"));
    assert!(
        unchecked.get("checksum").is_none(),
        "a command without validate must not claim a checksum: {unchecked}"
    );

    // A command that does declare one reports the algorithm that passed.
    let dev = dir.path().join("devices/crcdev");
    std::fs::create_dir_all(dev.join("commands")).unwrap();
    std::fs::write(
        dev.join("profile.yaml"),
        "schema: openbaud/profile@v0\nname: crcdev\nframing: { idle_ms: 20 }\n",
    )
    .unwrap();
    // mock:echo returns the frame verbatim, so a self-consistent crc frame validates.
    std::fs::write(
        dev.join("commands/probe.yaml"),
        r#"
schema: openbaud/command@v0
name: probe
frame: { hex: "01 04 00 00 00 01 {crc16_modbus}" }
response:
  match: { length: 8 }
  timeout_ms: 1000
  validate: { checksum: crc16_modbus }
  parse: { fields: { addr: { at: 0, type: u8 } } }
"#,
    )
    .unwrap();

    let checked = tools::call(
        "run_command",
        json!({ "device": "crcdev", "command": "probe", "port": "mock:echo" }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(checked["outcome"], json!("normal"), "got: {checked}");
    assert_eq!(checked["checksum"], json!("crc16_modbus"), "got: {checked}");
}

#[tokio::test]
async fn ui_binds_only_to_show_and_ask_tools() {
    let bound: Vec<(String, String)> = tools::list()
        .into_iter()
        .filter_map(|t| {
            let uri = t["_meta"]["ui"]["resourceUri"].as_str()?.to_string();
            Some((t["name"].as_str().unwrap().to_string(), uri))
        })
        .collect();

    // Data tools the agent calls routinely must never drag a UI along:
    // template binding is per-tool and unconditional, so a bound data tool
    // would pop a card on every internal call.
    for data_tool in
        ["list_ports", "open", "read", "request", "run_command", "run_workflow", "stream_poll"]
    {
        assert!(
            !bound.iter().any(|(n, _)| n == data_tool),
            "data tool {data_tool} must not bind a UI template"
        );
    }

    let find =
        |name: &str| -> Option<&str> { bound.iter().find(|(n, _)| n == name).map(|(_, u)| u.as_str()) };
    assert_eq!(find("ask_port"), Some("ui://openbaud/port-picker.html"));
    assert_eq!(find("show_result"), Some("ui://openbaud/viewer.html"));
    assert_eq!(bound.len(), 2, "only show/ask tools carry UI: {bound:?}");

    // Every bound uri must actually be served by resources/list.
    let served = openbaud::mcp::resources::list();
    let served: Vec<&str> =
        served["resources"].as_array().unwrap().iter().map(|r| r["uri"].as_str().unwrap()).collect();
    for (name, uri) in &bound {
        assert!(served.contains(&uri.as_str()), "tool {name} binds unserved uri {uri}");
    }
}

#[test]
fn enriched_ports_flag_profile_match_alias_and_use() {
    use openbaud::engine::transport::{enrich_ports, PortInfo};

    let ports = vec![
        PortInfo::usb("/dev/cu.usbmodem213101", "303A", "1001", Some("Espressif")),
        PortInfo::usb("/dev/tty.usbmodem213101", "303A", "1001", Some("Espressif")),
        PortInfo::usb("/dev/cu.usbserial-0001", "10C4", "EA60", None),
        PortInfo::mock(),
    ];
    let selector: openbaud_core::format::SelectorSpec =
        serde_json::from_value(json!({ "vid": "303A", "pid": "1001" })).unwrap();
    let devices = vec![("openbaud-pv-board".to_string(), Some(selector)), ("nosel".to_string(), None)];
    let open = vec![("/dev/cu.usbserial-0001".to_string(), "s1".to_string())];

    let enriched = serde_json::to_value(enrich_ports(ports, &devices, &open)).unwrap();
    let by_path = |p: &str| -> Value {
        enriched.as_array().unwrap().iter().find(|e| e["path"] == p).unwrap().clone()
    };

    // The selector match is real data, not a guess from the manufacturer string.
    assert_eq!(by_path("/dev/cu.usbmodem213101")["matches_devices"], json!(["openbaud-pv-board"]));
    // macOS exposes one physical port twice; the tty twin points at the cu canonical.
    assert_eq!(by_path("/dev/tty.usbmodem213101")["alias_of"], json!("/dev/cu.usbmodem213101"));
    assert!(by_path("/dev/cu.usbmodem213101").get("alias_of").is_none());
    // A port already held by a session must say so — this is the EBUSY case.
    assert_eq!(by_path("/dev/cu.usbserial-0001")["open_session"], json!("s1"));
    // Mock never matches a selector and carries no enrichment noise.
    assert!(by_path("mock:echo").get("matches_devices").is_none());
}

#[tokio::test]
async fn ask_port_returns_enriched_candidates_without_opening() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let result = tools::call("ask_port", json!({ "reason": "which board is the radar?" }), &ctx)
        .await
        .unwrap();

    assert_eq!(result["reason"], json!("which board is the radar?"));
    let candidates = result["candidates"].as_array().expect("candidates array");
    assert!(candidates.iter().any(|c| c["path"] == "mock:echo"), "got: {candidates:?}");
    // Asking must not touch hardware: no session may exist afterwards.
    assert!(ctx.sessions.get("s1").is_err(), "ask_port must not open anything");
}

#[tokio::test]
async fn show_result_hands_the_widget_a_resource_uri_not_the_payload() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    // Stand in for a spilled result from an earlier oversized tool call.
    let out = dir.path().join(".openbaud/out");
    std::fs::create_dir_all(&out).unwrap();
    let big: Vec<Value> = (0..36).map(|i| json!({ "angle_deg": i * 10, "distance_mm": 1000 + i })).collect();
    let payload = json!({ "outcome": "normal", "parsed": { "points": big } });
    std::fs::write(out.join("res-42-run_command.json"), serde_json::to_string(&payload).unwrap())
        .unwrap();

    let result = tools::call(
        "show_result",
        json!({ "path": ".openbaud/out/res-42-run_command.json" }),
        &ctx,
    )
    .await
    .unwrap();

    assert_eq!(result["source"], json!("file"));
    assert_eq!(result["uri"], json!("openbaud://result/res-42-run_command.json"));
    // The pointer must stay small — the payload belongs to the widget, not the model.
    assert!(
        serde_json::to_string(&result).unwrap().len() < 400,
        "show_result must not echo the payload: {result}"
    );

    // The widget fetches the full 36 points through the resource, bypassing the model.
    let read = openbaud::mcp::handle(
        "resources/read",
        json!({ "uri": "openbaud://result/res-42-run_command.json" }),
        &ctx,
    )
    .await
    .unwrap();
    let text = read["contents"][0]["text"].as_str().unwrap();
    let full: Value = serde_json::from_str(text).unwrap();
    assert_eq!(full["parsed"]["points"].as_array().unwrap().len(), 36);
}

#[tokio::test]
async fn result_resource_rejects_paths_outside_the_spill_dir() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    for bad in ["openbaud://result/../../etc/passwd", "openbaud://result/sub/dir.json"] {
        let err = openbaud::mcp::handle("resources/read", json!({ "uri": bad }), &ctx)
            .await
            .expect_err("traversal must be refused");
        assert_eq!(err.code, -32002, "uri {bad} got: {}", err.message);
    }
}

// ---------------------------------------------------------------------------
// Data tools: session_timeline, capture_frames, diagnose_frame, session_stats
// ---------------------------------------------------------------------------

fn write_obcap(dir: &std::path::Path, name: &str, lines: &[&str]) {
    let captures = dir.join("captures");
    std::fs::create_dir_all(&captures).unwrap();
    std::fs::write(captures.join(name), lines.join("\n") + "\n").unwrap();
}

#[tokio::test]
async fn session_timeline_folds_capture_and_audit_into_buckets_and_events() {
    let dir = scaffold_workspace();
    write_obcap(
        dir.path(),
        "t.obcap",
        &[
            r#"{"obcap":1,"session":"s7","port":"mock:echo","note":null,"started_ms":1000}"#,
            r#"{"ts_ms":1000,"dir":"tx","hex":"01 02"}"#,
            r#"{"ts_ms":1050,"dir":"rx","hex":"AA BB CC"}"#,
            r#"{"ts_ms":1900,"dir":"rx","hex":"DD"}"#,
        ],
    );
    std::fs::create_dir_all(dir.path().join(".openbaud")).unwrap();
    std::fs::write(
        dir.path().join(".openbaud/audit.jsonl"),
        concat!(
            r#"{"tool":"send","session":"s7","port":"mock:echo","tx_hex":"01 02","ok":true,"ts_ms":1005}"#,
            "\n",
            r#"{"tool":"run_command","device":"echodev","command":"ping","risk":"read","session":"s7","port":"mock:echo","outcome":"normal","ok":true,"ts_ms":1500}"#,
            "\n",
            r#"{"tool":"send","session":"OTHER","ok":true,"ts_ms":1600}"#,
            "\n",
            r#"{"tool":"run_workflow.step","device":"d","workflow":"w","phase":"step","session":"s7","port":"mock:echo","command":"c","ok":false,"detail":"boom","ts_ms":1800}"#,
            "\n",
        ),
    )
    .unwrap();
    let ctx = ctx_for(&dir);

    let result = tools::call(
        "session_timeline",
        json!({ "path": "captures/t.obcap", "buckets": 9 }),
        &ctx,
    )
    .await
    .unwrap();

    assert_eq!(result["view"]["kind"], json!("timeline"));
    assert_eq!(result["port"], json!("mock:echo"));
    assert_eq!(result["source"]["path"], json!("captures/t.obcap"));
    assert_eq!(result["span"], json!({ "from_ms": 1000, "to_ms": 1900 }));
    assert_eq!(result["bucket_ms"], json!(100));
    assert_eq!(
        result["density"],
        json!([
            { "t0": 1000, "tx_bytes": 2, "rx_bytes": 3 },
            { "t0": 1900, "tx_bytes": 0, "rx_bytes": 1 },
        ])
    );
    let kinds: Vec<&str> = result["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["write", "cmd", "workflow_step"], "other sessions are filtered out");
    assert_eq!(result["events"][1]["command"], json!("ping"));
    assert_eq!(result["events"][1]["outcome"], json!("normal"));
    assert_eq!(result["events"][2]["ok"], json!(false));
    assert_eq!(result["events"][2]["detail"], json!("boom"));

    // Explicit window narrows both density and events.
    let windowed = tools::call(
        "session_timeline",
        json!({ "path": "captures/t.obcap", "from_ms": 1400, "to_ms": 1600 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(windowed["density"], json!([]));
    let kinds: Vec<&str> = windowed["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["cmd"]);
}

#[tokio::test]
async fn session_timeline_span_is_never_derived_from_audit_events() {
    // Session ids restart at s1 every process, so the audit log accumulates
    // same-named events from other days. The natural span must come from the
    // capture alone; audit events are clipped to it, never allowed to widen it.
    let dir = scaffold_workspace();
    write_obcap(
        dir.path(),
        "t.obcap",
        &[
            r#"{"obcap":1,"session":"s6","port":"mock:echo","note":null,"started_ms":1000}"#,
            r#"{"ts_ms":1100,"dir":"tx","hex":"01"}"#,
            r#"{"ts_ms":1900,"dir":"rx","hex":"AA"}"#,
        ],
    );
    let three_days = 3 * 24 * 3600 * 1000u64;
    // Same session id, days after this capture — a different process's s6.
    let stale_future = format!(
        r#"{{"tool":"send","session":"s6","port":"mock:echo","ok":true,"ts_ms":{}}}"#,
        1000 + three_days
    );
    std::fs::create_dir_all(dir.path().join(".openbaud")).unwrap();
    std::fs::write(
        dir.path().join(".openbaud/audit.jsonl"),
        format!(
            "{}\n{}\n{stale_future}\n",
            // Same session id, recorded before this capture's epoch.
            r#"{"tool":"send","session":"s6","port":"mock:echo","ok":true,"ts_ms":500}"#,
            r#"{"tool":"send","session":"s6","port":"mock:echo","ok":true,"ts_ms":1200}"#,
        ),
    )
    .unwrap();
    let ctx = ctx_for(&dir);

    let result =
        tools::call("session_timeline", json!({ "path": "captures/t.obcap" }), &ctx).await.unwrap();
    assert_eq!(
        result["span"],
        json!({ "from_ms": 1000, "to_ms": 1900 }),
        "span must be the capture's own range, not stretched by stale audit epochs"
    );
    assert!(result["bucket_ms"].as_u64().unwrap() <= 5, "900 ms over 200 buckets");
    // Only the in-span event survives; the stale epochs are clipped out.
    let events = result["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "got: {events:?}");
    assert_eq!(events[0]["ts_ms"], json!(1200));

    // An explicit window still overrides the capture-derived span.
    let windowed = tools::call(
        "session_timeline",
        json!({ "path": "captures/t.obcap", "from_ms": 400, "to_ms": 600 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(windowed["span"], json!({ "from_ms": 400, "to_ms": 600 }));
    assert_eq!(windowed["events"].as_array().unwrap().len(), 1);
    assert_eq!(windowed["events"][0]["ts_ms"], json!(500));
}

#[tokio::test]
async fn session_timeline_rejects_session_ids_and_bad_paths() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    // v1 deliberately has no live-session mode; the refusal is loud.
    let err = tools::call("session_timeline", json!({ "session_id": "s1" }), &ctx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("record a capture first"), "got: {err:#}");

    for bad in [
        "captures/../../etc/passwd",
        "captures/sub/x.obcap",
        ".openbaud/out/res.json",
        "/etc/passwd",
    ] {
        let err = tools::call("session_timeline", json!({ "path": bad }), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("captures"), "path {bad} got: {err:#}");
    }

    // A file that is not an obcap capture is refused by its header, not guessed at.
    write_obcap(dir.path(), "bogus.obcap", &[r#"{"nope":true}"#]);
    let err = tools::call("session_timeline", json!({ "path": "captures/bogus.obcap" }), &ctx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("obcap"), "got: {err:#}");
}

#[tokio::test]
async fn capture_frames_reframes_idle_gaps_and_paginates() {
    let dir = scaffold_workspace();
    write_obcap(
        dir.path(),
        "f.obcap",
        &[
            r#"{"obcap":1,"session":"s1","port":"/dev/x","note":null,"started_ms":900}"#,
            r#"{"ts_ms":1000,"dir":"rx","hex":"01 02"}"#,
            r#"{"ts_ms":1005,"dir":"rx","hex":"03"}"#,
            r#"{"ts_ms":1100,"dir":"rx","hex":"AA BB"}"#,
            r#"{"ts_ms":1150,"dir":"tx","hex":"FF"}"#,
            r#"{"ts_ms":1200,"dir":"rx","hex":"CC"}"#,
        ],
    );
    let ctx = ctx_for(&dir);

    let page1 = tools::call(
        "capture_frames",
        json!({ "path": "captures/f.obcap", "framing": { "idle_ms": 30 }, "max_frames": 2 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(page1["view"]["kind"], json!("capture"));
    assert_eq!(page1["header"], json!({ "port": "/dev/x", "started_ms": 900 }));
    assert_eq!(page1["source"]["path"], json!("captures/f.obcap"));
    assert_eq!(page1["total_in_window"], json!(4));
    assert_eq!(page1["next_cursor"], json!(2));
    assert_eq!(
        page1["frames"],
        json!([
            { "seq": 0, "ts_ms": 1000, "dir": "rx", "hex": "01 02 03", "len": 3 },
            { "seq": 1, "ts_ms": 1100, "dir": "rx", "hex": "AA BB", "len": 2 },
        ])
    );

    let page2 = tools::call(
        "capture_frames",
        json!({
            "path": "captures/f.obcap",
            "framing": { "idle_ms": 30 },
            "max_frames": 2,
            "cursor": 2,
        }),
        &ctx,
    )
    .await
    .unwrap();
    assert!(page2.get("next_cursor").is_none(), "last page carries no cursor: {page2}");
    assert_eq!(
        page2["frames"],
        json!([
            { "seq": 2, "ts_ms": 1150, "dir": "tx", "hex": "FF", "len": 1 },
            { "seq": 3, "ts_ms": 1200, "dir": "rx", "hex": "CC", "len": 1 },
        ])
    );

    // The rx boundaries must equal a hand-driven Deframer replay of the same
    // records under the same gap rule.
    let mut deframer = Deframer::new(Framing::Idle { idle_ms: 30 });
    let mut manual: Vec<String> = Vec::new();
    let mut last: Option<u64> = None;
    let rx_records: [(u64, Vec<u8>); 4] = [
        (1000, vec![0x01, 0x02]),
        (1005, vec![0x03]),
        (1100, vec![0xAA, 0xBB]),
        (1200, vec![0xCC]),
    ];
    for (ts, bytes) in rx_records {
        if let Some(prev) = last {
            if ts - prev >= 30 {
                if let Some(f) = deframer.flush_pending() {
                    manual.push(openbaud_core::hex::to_hex(&f));
                }
            }
        }
        for f in deframer.push(&bytes) {
            manual.push(openbaud_core::hex::to_hex(&f));
        }
        last = Some(ts);
    }
    if let Some(f) = deframer.flush_pending() {
        manual.push(openbaud_core::hex::to_hex(&f));
    }
    let tool_rx: Vec<String> = [&page1, &page2]
        .iter()
        .flat_map(|p| p["frames"].as_array().unwrap().clone())
        .filter(|f| f["dir"] == json!("rx"))
        .map(|f| f["hex"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(tool_rx, manual, "tool framing must match a direct Deframer replay");

    // Window filter counts only in-window frames.
    let windowed = tools::call(
        "capture_frames",
        json!({
            "path": "captures/f.obcap",
            "framing": { "idle_ms": 30 },
            "from_ms": 1100,
            "to_ms": 1150,
        }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(windowed["total_in_window"], json!(2));
}

#[tokio::test]
async fn capture_frames_requires_a_framing_and_a_known_device() {
    let dir = scaffold_workspace();
    write_obcap(
        dir.path(),
        "f.obcap",
        &[
            r#"{"obcap":1,"session":"s1","port":"p","note":null,"started_ms":1}"#,
            r#"{"ts_ms":2,"dir":"rx","hex":"41 0A"}"#,
        ],
    );
    let ctx = ctx_for(&dir);

    // Neither framing nor device: loud, never a silent default.
    let err = tools::call("capture_frames", json!({ "path": "captures/f.obcap" }), &ctx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("framing"), "got: {err:#}");

    // Unknown device is the workspace's loud lookup error.
    let err = tools::call(
        "capture_frames",
        json!({ "path": "captures/f.obcap", "device": "nope" }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("no device"), "got: {err:#}");

    // An explicit delimiter framing frames without any idle logic.
    let framed = tools::call(
        "capture_frames",
        json!({ "path": "captures/f.obcap", "framing": { "delimiter": "\n" } }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(
        framed["frames"],
        json!([{ "seq": 0, "ts_ms": 2, "dir": "rx", "hex": "41", "len": 1 }])
    );
    assert_eq!(framed["total_in_window"], json!(1));
}

#[tokio::test]
async fn diagnose_frame_checksum_matrix_reports_hits_and_misses() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    // "123456789" + its CRC-16/MODBUS tail (little-endian 37 4B).
    let mut frame = b"123456789".to_vec();
    frame.extend(openbaud_core::checksum::ChecksumKind::Crc16Modbus.compute(b"123456789"));
    let result = tools::call(
        "diagnose_frame",
        json!({ "hex": openbaud_core::hex::to_hex(&frame) }),
        &ctx,
    )
    .await
    .unwrap();

    assert_eq!(result["view"]["kind"], json!("diagnostics"));
    assert_eq!(result["frame_len"], json!(11));
    assert_eq!(result["hex"], json!(openbaud_core::hex::to_hex(&frame)));
    let matrix = result["checksum_matrix"].as_array().unwrap();
    assert_eq!(matrix.len(), 14, "7 algorithms x 2 encodings");

    let row = |kind: &str, encoding: &str| -> &Value {
        matrix
            .iter()
            .find(|r| r["kind"] == json!(kind) && r["encoding"] == json!(encoding))
            .unwrap()
    };
    let hit = row("crc16_modbus", "raw");
    assert_eq!(hit["ok"], json!(true));
    assert_eq!(hit["computed"], json!("37 4B"));
    // `at` is the byte offset where the stored checksum starts:
    // frame_len - stored_len (11 - 2 raw crc16 bytes).
    assert_eq!(hit["at"], json!(9));

    let miss = row("xor8", "raw");
    assert_eq!(miss["ok"], json!(false));
    assert_eq!(miss["at"], json!(10), "1 raw xor8 byte at the tail of 11");
    assert!(miss["expected"].is_string() && miss["actual"].is_string(), "got: {miss}");

    // ascii_hex doubles the stored length: on "AB12" (4 ASCII bytes) a 1-byte
    // xor8 stored as 2 hex characters starts at byte 2.
    let ascii_result = tools::call("diagnose_frame", json!({ "hex": "41 42 31 32" }), &ctx)
        .await
        .unwrap();
    let ascii = ascii_result["checksum_matrix"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == json!("xor8") && r["encoding"] == json!("ascii_hex"))
        .unwrap();
    assert_eq!(ascii["at"], json!(2), "got: {ascii}");
    assert_eq!(ascii["ok"], json!(false), "xor8(\"AB\") is 03, stored says 12: {ascii}");

    // Invalid hex is a loud caller error, not an empty matrix.
    let err = tools::call("diagnose_frame", json!({ "hex": "ZZ" }), &ctx).await.unwrap_err();
    assert!(err.to_string().contains("hex"), "got: {err:#}");
}

#[tokio::test]
async fn diagnose_frame_omits_at_when_the_algorithm_cannot_fit() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    // A 1-byte frame cannot carry a crc32 (4 raw bytes): the row explains why
    // and carries no fabricated position.
    let result = tools::call("diagnose_frame", json!({ "hex": "01" }), &ctx).await.unwrap();
    let matrix = result["checksum_matrix"].as_array().unwrap();
    let row = |kind: &str, encoding: &str| -> &Value {
        matrix
            .iter()
            .find(|r| r["kind"] == json!(kind) && r["encoding"] == json!(encoding))
            .unwrap()
    };
    for (kind, encoding) in [("crc32", "raw"), ("crc16_modbus", "raw"), ("crc32", "ascii_hex")] {
        let r = row(kind, encoding);
        assert!(r["error"].is_string(), "got: {r}");
        assert!(r.get("at").is_none(), "an inapplicable row carries no at: {r}");
        assert!(r.get("ok").is_none(), "an inapplicable row carries no verdict: {r}");
    }
}

#[tokio::test]
async fn diagnose_frame_probes_expected_command_parse_at_offsets() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    // echodev/ping parses { y: { at: 1, type: u8 } }; "AB 42 ED" fits at offset 0.
    let result = tools::call(
        "diagnose_frame",
        json!({ "hex": "AB 42 ED", "expected": { "device": "echodev", "command": "ping" } }),
        &ctx,
    )
    .await
    .unwrap();

    let attempts = result["parse_attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 5, "offsets -2..=+2");
    let at = |off: i64| -> &Value {
        attempts.iter().find(|a| a["offset"] == json!(off)).unwrap()
    };
    // The verdict field is named `parsed`: structurally decodable at that
    // offset, deliberately not `ok` — the offsets are mutually exclusive
    // hypotheses, so a parsed row is not "the right answer".
    assert_eq!(at(0)["parsed"], json!(true));
    assert!(at(0).get("ok").is_none(), "the verdict field is parsed, not ok: {}", at(0));
    assert_eq!(at(0)["fields"]["y"], json!(0x42));
    assert_eq!(at(-2)["parsed"], json!(false), "field would sit before byte 0: {}", at(-2));
    assert_eq!(at(2)["parsed"], json!(false), "field would sit past the frame: {}", at(2));
    assert!(at(2)["error"].is_string());

    // Unknown device / command are the workspace's loud lookup errors.
    let err = tools::call(
        "diagnose_frame",
        json!({ "hex": "AB", "expected": { "device": "ghost", "command": "ping" } }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("no device"), "got: {err:#}");
}

#[tokio::test]
async fn session_stats_reports_live_counters_and_capture_state() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);

    let opened = tools::call("open", json!({ "port": "mock:echo", "baud": 9600 }), &ctx)
        .await
        .unwrap();
    let sid = opened["session_id"].as_str().unwrap().to_string();
    tools::call("send", json!({ "session_id": sid, "hex": "01 02 03" }), &ctx).await.unwrap();
    tools::call("read", json!({ "session_id": sid, "timeout_ms": 1000 }), &ctx).await.unwrap();

    let stats = tools::call("session_stats", json!({}), &ctx).await.unwrap();
    let sessions = stats["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    let s = &sessions[0];
    assert_eq!(s["session_id"], json!(sid));
    assert_eq!(s["port"], json!("mock:echo"));
    assert_eq!(s["tx_bytes"], json!(3));
    assert_eq!(s["rx_bytes"], json!(3), "echo returns every byte");
    assert!(s["opened_ms"].as_u64().unwrap() > 0);
    assert!(s["framing"].as_str().unwrap().contains("Idle"), "got: {s}");
    assert_eq!(s["transport"]["baud"], json!(9600));
    assert_eq!(s["dropped_bytes"], json!(0));
    assert!(s["chunks_seen"].as_u64().unwrap() >= 1);
    assert!(s["last_rx_ms"].as_u64().unwrap() > 0);
    assert_eq!(s["capture"], json!({ "active": false }));

    // With a capture running the entry names the file and its live counters.
    let started = tools::call("capture_start", json!({ "session_id": sid }), &ctx).await.unwrap();
    tools::call("send", json!({ "session_id": sid, "hex": "AA" }), &ctx).await.unwrap();
    let stats = tools::call("session_stats", json!({ "session_id": sid }), &ctx).await.unwrap();
    let s = &stats["sessions"][0];
    assert_eq!(s["capture"]["active"], json!(true));
    assert_eq!(s["capture"]["path"], started["abs_path"]);
    assert!(s["capture"]["chunks"].as_u64().unwrap() >= 1);
    assert_eq!(s["tx_bytes"], json!(4));

    // Unknown ids and closed sessions are loud / absent, not guessed.
    let err = tools::call("session_stats", json!({ "session_id": "zzz" }), &ctx).await.unwrap_err();
    assert!(err.to_string().contains("no session"), "got: {err:#}");
    tools::call("close", json!({ "session_id": sid }), &ctx).await.unwrap();
    let stats = tools::call("session_stats", json!({}), &ctx).await.unwrap();
    assert_eq!(stats["sessions"], json!([]));
}

// ---------------------------------------------------------------------------
// stream_poll: per-consumer frame subscriptions (R-07)
// ---------------------------------------------------------------------------

async fn open_echo_lines(ctx: &Arc<Ctx>) -> String {
    let opened = tools::call(
        "open",
        json!({ "port": "mock:echo", "framing": { "delimiter": "\n" } }),
        ctx,
    )
    .await
    .unwrap();
    opened["session_id"].as_str().unwrap().to_string()
}

/// stream_poll never waits, so tests poll (without acking) until the echo has
/// been deframed up to `want` frames — loudly failing after a deadline.
async fn poll_until_next_seq(ctx: &Arc<Ctx>, sub_id: &str, want: u64, max_frames: u64) -> Value {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let result = tools::call(
            "stream_poll",
            json!({ "subscription_id": sub_id, "max_frames": max_frames }),
            ctx,
        )
        .await
        .unwrap();
        if result["next_seq"].as_u64().unwrap() >= want {
            return result;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "subscription {sub_id} never reached seq {want}: {result}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

fn frame_texts(result: &Value) -> Vec<&str> {
    result["frames"].as_array().unwrap().iter().map(|f| f["text"].as_str().unwrap()).collect()
}

fn frame_seqs(result: &Value) -> Vec<u64> {
    result["frames"].as_array().unwrap().iter().map(|f| f["seq"].as_u64().unwrap()).collect()
}

#[tokio::test]
async fn stream_poll_subscribes_and_pulls_echo_frames_via_rpc() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    tools::call("send", json!({ "session_id": sid, "text": "a\nb\n" }), &ctx).await.unwrap();

    // Creation goes through the full JSON-RPC layer, like an MCP host would.
    let created = openbaud::mcp::handle(
        "tools/call",
        json!({ "name": "stream_poll", "arguments": { "session_id": sid } }),
        &ctx,
    )
    .await
    .unwrap();
    assert!(created.get("isError").is_none(), "got: {created}");
    let structured = &created["structuredContent"];
    let sub_id = structured["subscription_id"].as_str().expect("subscription_id").to_string();
    assert_eq!(structured["session_id"], json!(sid));

    let result = poll_until_next_seq(&ctx, &sub_id, 2, 64).await;
    assert_eq!(frame_texts(&result), vec!["a", "b"]);
    assert_eq!(frame_seqs(&result), vec![0, 1], "seq starts at 0 and is monotonic");
    assert_eq!(result["next_seq"], json!(2));
    assert_eq!(result["dropped_frames"], json!(0));
    // Every result folds in the session's live counters.
    let stats = &result["stats"];
    assert!(stats["rx_bytes"].as_u64().unwrap() >= 4, "got: {stats}");
    assert!(stats["tx_bytes"].as_u64().unwrap() >= 4, "got: {stats}");
    assert!(stats["last_rx_ms"].as_u64().unwrap() > 0, "got: {stats}");
    assert!(stats["buffered"].is_number() && stats["dropped_bytes"].is_number(), "got: {stats}");
    // ts_ms/hex/text carry the same shape read frames do.
    assert_eq!(result["frames"][0]["hex"], json!("61"));
    assert!(result["frames"][0]["ts_ms"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn stream_subscriptions_and_read_do_not_steal_frames() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    tools::call("send", json!({ "session_id": sid, "text": "x\ny\n" }), &ctx).await.unwrap();

    let a = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let b = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let a = a["subscription_id"].as_str().unwrap().to_string();
    let b = b["subscription_id"].as_str().unwrap().to_string();
    assert_ne!(a, b);

    // Risk ①: both subscriptions and the shared read cursor each see every
    // frame — nobody steals from anybody.
    let ra = poll_until_next_seq(&ctx, &a, 2, 64).await;
    let rb = poll_until_next_seq(&ctx, &b, 2, 64).await;
    assert_eq!(frame_texts(&ra), vec!["x", "y"]);
    assert_eq!(frame_texts(&rb), vec!["x", "y"]);
    assert_eq!(frame_seqs(&ra), vec![0, 1]);
    assert_eq!(frame_seqs(&rb), vec![0, 1]);

    let read = tools::call("read", json!({ "session_id": sid, "timeout_ms": 1000 }), &ctx)
        .await
        .unwrap();
    let read_texts: Vec<&str> =
        read["frames"].as_array().unwrap().iter().map(|f| f["text"].as_str().unwrap()).collect();
    assert_eq!(read_texts, vec!["x", "y"], "read gets its own copy of every frame");

    // And the read did not consume the subscriptions' unacked frames.
    let again = tools::call("stream_poll", json!({ "subscription_id": a }), &ctx).await.unwrap();
    assert_eq!(frame_texts(&again), vec!["x", "y"]);
}

#[tokio::test]
async fn stream_poll_ack_releases_and_redelivers() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    tools::call("send", json!({ "session_id": sid, "text": "a\nb\n" }), &ctx).await.unwrap();
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();

    let first = poll_until_next_seq(&ctx, &sub, 2, 64).await;
    // No since_seq: idempotent re-read — the same unacked frames come back.
    let second = tools::call("stream_poll", json!({ "subscription_id": sub }), &ctx).await.unwrap();
    assert_eq!(second["frames"], first["frames"], "unacked frames are redelivered unchanged");

    // since_seq releases everything below it.
    let acked = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 1 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(frame_seqs(&acked), vec![1]);
    assert_eq!(acked["next_seq"], json!(2));

    let done = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 2 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(done["frames"], json!([]));
    assert_eq!(done["next_seq"], json!(2));

    // Acking frames that were never delivered is a loud caller error.
    let err = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 99 }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("never delivered"), "got: {err:#}");
}

#[tokio::test]
async fn stream_poll_overflow_drops_oldest_and_counts() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();

    // More frames than one subscription retains: the oldest are dropped and
    // counted (risk ②), never silently.
    let total = (MAX_RETAINED_FRAMES + 76) as u64;
    let payload: String = (0..total).map(|i| format!("{i}\n")).collect();
    tools::call("send", json!({ "session_id": sid, "text": payload }), &ctx).await.unwrap();

    let result = poll_until_next_seq(&ctx, &sub, total, 256).await;
    assert_eq!(result["dropped_frames"], json!(76));
    assert_eq!(result["next_seq"], json!(total));
    assert_eq!(result["frames"].as_array().unwrap().len(), 256, "max_frames caps the page");
    assert_eq!(result["frames"][0]["seq"], json!(76), "oldest retained frame comes first");
    assert_eq!(result["frames"][0]["text"], json!("76"));

    // since_seq pointing into the dropped range releases nothing and delivery
    // starts from the oldest retained frame — the drop is never papered over.
    let replay = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 10 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(replay["frames"][0]["seq"], json!(76));
    assert_eq!(replay["dropped_frames"], json!(76));
}

#[tokio::test]
async fn stream_poll_over_ack_beyond_delivered_page_is_loud() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();
    tools::call("send", json!({ "session_id": sid, "text": "a\nb\nc\nd\ne\n" }), &ctx)
        .await
        .unwrap();

    // A max_frames-capped page: all 5 frames drained (next_seq = 5), but only
    // seqs 0 and 1 were ever delivered.
    let page = poll_until_next_seq(&ctx, &sub, 5, 2).await;
    assert_eq!(frame_seqs(&page), vec![0, 1]);
    assert_eq!(page["next_seq"], json!(5));

    // Acking with next_seq would silently destroy seqs 2..4, which the
    // consumer never saw — that must be a loud error, not a release.
    let err = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 5 }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("never delivered"), "got: {err:#}");

    // The correct ack — last delivered seq + 1 — releases only the page, and
    // the undelivered frames are still there.
    let rest = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 2, "max_frames": 64 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(frame_seqs(&rest), vec![2, 3, 4], "undelivered frames must survive the ack");
    assert_eq!(frame_texts(&rest), vec!["c", "d", "e"]);
}

#[tokio::test]
async fn stream_poll_surfaces_dead_port_once_drained() {
    // A port that delivers two lines and then dies (EOF).
    let (client, server) = tokio::io::duplex(4096);
    let session = Session::spawn(
        "dead1".into(),
        "mock:dying".into(),
        Framing::Delimiter { delimiter: vec![b'\n'] },
        Box::new(client),
    );
    let (_sread, mut swrite) = tokio::io::split(server);
    swrite.write_all(b"hello\nworld\n").await.unwrap();
    drop(swrite);
    drop(_sread);

    // Wait until the reader has seen the EOF and recorded the port error.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while session.stats().get("error").is_none() {
        assert!(tokio::time::Instant::now() < deadline, "port error never surfaced");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // Buffered frames are still delivered after death — same rule as read.
    let now = openbaud::engine::now_ms();
    let sub = session.stream_subscribe(now, None).unwrap();
    let result = session.stream_pull(&sub, None, 64, DEFAULT_POLL_INLINE_BYTES, now).unwrap();
    let texts: Vec<&str> = result["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec!["hello", "world"]);

    // Once nothing retained is left to deliver, a poll on the dead port is a
    // loud error, never an endless stream of healthy-looking empty results.
    let err = session.stream_pull(&sub, Some(2), 64, DEFAULT_POLL_INLINE_BYTES, now).unwrap_err();
    assert!(err.to_string().contains("port failed"), "got: {err:#}");
}

#[tokio::test]
async fn stream_poll_counts_buffer_overflow_gap_per_subscription() {
    // Flood the session's shared byte buffer (1 MiB cap) past an unpolled
    // subscription's cursor: the loss must be attributed to the subscription
    // as dropped_chunks — dropped_frames == 0 alone must never mean "no loss".
    let (client, server) = tokio::io::duplex(256 * 1024);
    let session = Session::spawn(
        "ovf1".into(),
        "mock:flood".into(),
        Framing::Delimiter { delimiter: vec![b'\n'] },
        Box::new(client),
    );
    let now = openbaud::engine::now_ms();
    let sub = session.stream_subscribe(now, None).unwrap();

    // ~1.13 MiB of 64-byte lines, never polled while they arrive.
    let line = {
        let mut l = vec![b'x'; 63];
        l.push(b'\n');
        l
    };
    let (_sread, mut swrite) = tokio::io::split(server);
    for _ in 0..(1024 * 1024 / 64 + 2048) {
        swrite.write_all(&line).await.unwrap();
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while session.stream_stats()["dropped_bytes"].as_u64().unwrap() == 0 {
        assert!(tokio::time::Instant::now() < deadline, "byte overflow never happened");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // 256 63-byte frames render past the default budget, so give this page
    // the maximum inline budget — the assertion below is about max_frames.
    let result = session
        .stream_pull(&sub, None, 256, MAX_POLL_INLINE_BYTES, openbaud::engine::now_ms())
        .unwrap();
    assert!(
        result["dropped_chunks"].as_u64().unwrap() > 0,
        "chunk-buffer overflow past the cursor must be counted per subscription: {result}"
    );
    assert!(
        result["stats"]["dropped_bytes"].as_u64().unwrap() > 0,
        "session stats must also carry the byte loss: {result}"
    );
    // The post-gap bytes overran the retention queue too — also counted.
    assert!(result["dropped_frames"].as_u64().unwrap() > 0, "got: {result}");
    assert_eq!(result["frames"].as_array().unwrap().len(), 256);
}

#[tokio::test]
async fn stream_poll_close_and_session_close_invalidate_ids() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;

    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();
    let closed = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "close": true }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(closed["closed"], json!(true));
    assert_eq!(closed["subscription_id"], json!(sub));
    assert!(closed.get("frames").is_none(), "a close result returns no frames: {closed}");
    assert!(
        closed["stats"]["rx_bytes"].is_u64(),
        "every result, close included, folds in the session stats: {closed}"
    );

    let err = tools::call("stream_poll", json!({ "subscription_id": sub }), &ctx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no stream subscription"), "got: {err:#}");

    // Closing the session releases every subscription with it (risk ③).
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();
    tools::call("close", json!({ "session_id": sid }), &ctx).await.unwrap();
    let err = tools::call("stream_poll", json!({ "subscription_id": sub }), &ctx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no stream subscription"), "got: {err:#}");
}

#[tokio::test]
async fn stream_poll_subscription_cap_is_loud() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;

    for i in 0..MAX_SUBSCRIPTIONS_PER_SESSION {
        tools::call("stream_poll", json!({ "session_id": sid }), &ctx)
            .await
            .unwrap_or_else(|e| panic!("subscription {i} should fit under the cap: {e:#}"));
    }
    let err = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains(&MAX_SUBSCRIPTIONS_PER_SESSION.to_string()) && msg.contains("close"),
        "got: {msg}"
    );
}

#[tokio::test]
async fn stream_subscription_ttl_sweeps_idle_subscribers() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let session = ctx.sessions.get(&sid).unwrap();
    let now = openbaud::engine::now_ms();

    // Any later stream call on the session sweeps subscriptions idle past the
    // TTL — driven here with a fabricated clock instead of a 120 s sleep.
    let stale = session.stream_subscribe(now, None).unwrap();
    let fresh = session.stream_subscribe(now + SUBSCRIPTION_IDLE_TTL_MS + 1, None).unwrap();
    assert!(!session.has_stream_subscription(&stale), "stale subscription must be swept");
    assert!(session.has_stream_subscription(&fresh));
    let err = tools::call("stream_poll", json!({ "subscription_id": stale }), &ctx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no stream subscription"), "got: {err:#}");

    // Pulling a subscription that itself sat idle past the TTL expires it loudly.
    let err = session
        .stream_pull(
            &fresh,
            None,
            64,
            DEFAULT_POLL_INLINE_BYTES,
            now + 2 * (SUBSCRIPTION_IDLE_TTL_MS + 1),
        )
        .unwrap_err();
    assert!(err.to_string().contains("expired"), "got: {err:#}");
    assert!(!session.has_stream_subscription(&fresh));

    // The explicit sweep seam reports what it removed.
    let third = session.stream_subscribe(now, None).unwrap();
    let swept = session.sweep_stream_subscriptions_at(now + SUBSCRIPTION_IDLE_TTL_MS + 1);
    assert_eq!(swept, vec![third]);
}

#[tokio::test]
async fn stream_poll_parameter_errors_are_loud() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let sid2 = open_echo_lines(&ctx).await;

    // Neither id: nothing to create and nothing to poll.
    let err = tools::call("stream_poll", json!({}), &ctx).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("session_id") && msg.contains("subscription_id"), "got: {msg}");

    let err = tools::call("stream_poll", json!({ "session_id": "zzz" }), &ctx).await.unwrap_err();
    assert!(err.to_string().contains("no session"), "got: {err:#}");
    let err = tools::call("stream_poll", json!({ "subscription_id": "zzz" }), &ctx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no stream subscription"), "got: {err:#}");

    // A fresh subscription on an untouched session: nothing received yet, so
    // last_rx_ms is null (never 0), and the retention is empty.
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    assert!(created["stats"]["last_rx_ms"].is_null(), "got: {created}");
    assert_eq!(created["frames"], json!([]));
    let sub = created["subscription_id"].as_str().unwrap().to_string();

    // since_seq / close only make sense against an existing subscription.
    let err = tools::call("stream_poll", json!({ "session_id": sid, "since_seq": 0 }), &ctx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("since_seq"), "got: {err:#}");
    let err = tools::call("stream_poll", json!({ "session_id": sid, "close": true }), &ctx)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("close"), "got: {err:#}");

    // max_frames beyond the cap, and a session/subscription mismatch.
    let err = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "max_frames": 300 }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("256"), "got: {err:#}");
    let err = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "max_frames": 0 }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("1..=256"), "got: {err:#}");
    // Out-of-range u64 is rejected as the value sent, never truncated first.
    let err = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "max_frames": 4_294_967_301u64 }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("4294967301"), "got: {err:#}");
    // Ack and close contradict each other: closing releases everything anyway.
    let err = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "close": true, "since_seq": 0 }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("since_seq"), "got: {err:#}");
    let err = tools::call(
        "stream_poll",
        json!({ "session_id": sid2, "subscription_id": sub }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("belongs"), "got: {err:#}");
}

// ---------------------------------------------------------------------------
// stream_poll: per-frame parsing (parse resolved at subscribe time)
// ---------------------------------------------------------------------------

/// A command with a parse block carrying a unit, written into the scaffold
/// workspace by the parse tests themselves so the shared scaffold stays
/// untouched. `mv` decodes byte 1 as u8 — a one-byte frame cannot satisfy it,
/// which is exactly what the parse_error tests need.
fn write_volt_command(dir: &tempfile::TempDir) {
    std::fs::write(
        dir.path().join("devices/echodev/commands/volt.yaml"),
        r#"
schema: openbaud/command@v0
name: volt
frame: { hex: "56" }
response:
  match: { length: 2 }
  parse: { fields: { mv: { at: 1, type: u8, unit: "mV" } } }
"#,
    )
    .unwrap();
}

#[tokio::test]
async fn stream_poll_parse_attaches_parsed_fields_per_frame() {
    let dir = scaffold_workspace();
    write_volt_command(&dir);
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;

    let created = tools::call(
        "stream_poll",
        json!({ "session_id": sid, "parse": { "device": "echodev", "command": "volt" } }),
        &ctx,
    )
    .await
    .unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();
    // The result echoes that parse is in effect, from creation on.
    assert_eq!(created["parse"], json!({ "device": "echodev", "command": "volt" }));

    tools::call("send", json!({ "session_id": sid, "text": "AB\n" }), &ctx).await.unwrap();
    let result = poll_until_next_seq(&ctx, &sub, 1, 64).await;
    assert_eq!(result["parse"], json!({ "device": "echodev", "command": "volt" }));
    // Units travel with the result, same semantics as run_command.
    assert_eq!(result["units"], json!({ "mv": "mV" }));
    let frame = &result["frames"][0];
    assert_eq!(frame["parsed"], json!({ "mv": 0x42 }));
    assert!(frame.get("parse_error").is_none(), "got: {frame}");
    // The original frame shape is unchanged next to `parsed`.
    assert_eq!(frame["seq"], json!(0));
    assert_eq!(frame["hex"], json!("41 42"));
    assert_eq!(frame["text"], json!("AB"));
    assert!(frame["ts_ms"].as_u64().unwrap() > 0);

    // Idempotent redelivery (no ack) carries the same parsed values — the
    // bytes were parsed once when the frame was retained, not per delivery.
    let again = tools::call("stream_poll", json!({ "subscription_id": sub }), &ctx).await.unwrap();
    assert_eq!(again["frames"][0]["parsed"], json!({ "mv": 0x42 }));
}

#[tokio::test]
async fn stream_poll_parse_error_is_per_frame_and_stream_continues() {
    let dir = scaffold_workspace();
    write_volt_command(&dir);
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let created = tools::call(
        "stream_poll",
        json!({ "session_id": sid, "parse": { "device": "echodev", "command": "volt" } }),
        &ctx,
    )
    .await
    .unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();

    // Frame "a" is one byte — `mv` at offset 1 cannot decode — then "QR"
    // parses fine: one bad frame never interrupts the stream.
    tools::call("send", json!({ "session_id": sid, "text": "a\nQR\n" }), &ctx).await.unwrap();
    let result = poll_until_next_seq(&ctx, &sub, 2, 64).await;
    let frames = result["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2, "got: {result}");
    let bad = &frames[0];
    assert!(bad.get("parsed").is_none(), "got: {bad}");
    assert!(
        !bad["parse_error"].as_str().unwrap().is_empty(),
        "bad frame must carry a parse_error reason, got: {bad}"
    );
    assert_eq!(bad["text"], json!("a"), "raw frame shape survives a parse failure");
    let good = &frames[1];
    assert_eq!(good["parsed"], json!({ "mv": 0x52 }));
    assert!(good.get("parse_error").is_none(), "got: {good}");
}

#[tokio::test]
async fn stream_poll_parse_creation_errors_are_loud() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;

    // Unknown device.
    let err = tools::call(
        "stream_poll",
        json!({ "session_id": sid, "parse": { "device": "nodev", "command": "ping" } }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("no device"), "got: {err:#}");

    // Unknown command.
    let err = tools::call(
        "stream_poll",
        json!({ "session_id": sid, "parse": { "device": "echodev", "command": "zzz" } }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("no command"), "got: {err:#}");

    // Command without a response.parse block (wipe has no response at all).
    let err = tools::call(
        "stream_poll",
        json!({ "session_id": sid, "parse": { "device": "echodev", "command": "wipe" } }),
        &ctx,
    )
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("response.parse") && msg.contains("wipe"), "got: {msg}");

    // Malformed parse argument shapes.
    let err = tools::call(
        "stream_poll",
        json!({ "session_id": sid, "parse": "echodev/ping" }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("parse"), "got: {err:#}");
    let err = tools::call(
        "stream_poll",
        json!({ "session_id": sid, "parse": { "device": "echodev" } }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("command"), "got: {err:#}");

    // None of the failed creations may have leaked a subscription.
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    assert!(
        created["subscription_id"].as_str().unwrap().ends_with("-sub1"),
        "failed parse creations must not consume subscription slots, got: {created}"
    );
}

#[tokio::test]
async fn stream_poll_parse_on_follow_up_poll_is_loud_and_no_parse_is_unchanged() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();
    // A subscription created without parse advertises none.
    assert!(created.get("parse").is_none(), "got: {created}");

    // parse belongs to creation only — a follow-up poll cannot smuggle it in.
    let err = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "parse": { "device": "echodev", "command": "ping" } }),
        &ctx,
    )
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("parse") && msg.contains("creat"), "got: {msg}");

    // Zero regression without parse: frames carry neither parsed nor
    // parse_error, and the result carries neither parse nor units.
    tools::call("send", json!({ "session_id": sid, "text": "AB\n" }), &ctx).await.unwrap();
    let result = poll_until_next_seq(&ctx, &sub, 1, 64).await;
    assert!(result.get("parse").is_none(), "got: {result}");
    assert!(result.get("units").is_none(), "got: {result}");
    let frame = &result["frames"][0];
    assert!(frame.get("parsed").is_none() && frame.get("parse_error").is_none(), "got: {frame}");
}

// ---------------------------------------------------------------------------
// stream_poll: inline byte budget (max_inline_bytes)
// ---------------------------------------------------------------------------

/// Byte-budgeted flavor of `poll_until_next_seq`: poll with a fixed
/// `max_inline_bytes` (never acking) until the subscription has deframed up to
/// `want` frames — loudly failing after a deadline.
async fn poll_budget_until_next_seq(ctx: &Arc<Ctx>, sub_id: &str, want: u64, budget: u64) -> Value {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let result = tools::call(
            "stream_poll",
            json!({ "subscription_id": sub_id, "max_inline_bytes": budget }),
            ctx,
        )
        .await
        .unwrap();
        if result["next_seq"].as_u64().unwrap() >= want {
            return result;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "subscription {sub_id} never reached seq {want}: {result}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn stream_poll_inline_byte_budget_paginates_whole_frames() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();

    // Four 100-byte frames. Each renders as 299 hex chars ("XX " joined) plus
    // 100 text chars = 399 metered bytes; the budget counts hex+text only.
    let payload = format!("{}\n", "x".repeat(100)).repeat(4);
    tools::call("send", json!({ "session_id": sid, "text": payload }), &ctx).await.unwrap();

    // Budget 900: exactly two whole frames fit (798); the third (1197) would
    // overflow, so the page stops at a frame boundary — never a partial frame.
    let page = poll_budget_until_next_seq(&ctx, &sub, 4, 900).await;
    assert_eq!(frame_seqs(&page), vec![0, 1], "page holds only whole frames within budget");
    assert_eq!(page["page_bytes"], json!(798));
    assert_eq!(page["oversized_frame"], json!(false));
    assert_eq!(page["next_seq"], json!(4), "draining is not capped by the page budget");

    // Ack the delivered page: the remaining frames arrive on the next page.
    let rest = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 2, "max_inline_bytes": 900 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(frame_seqs(&rest), vec![2, 3]);
    assert_eq!(rest["page_bytes"], json!(798));
    assert_eq!(rest["oversized_frame"], json!(false));
}

#[tokio::test]
async fn stream_poll_oversized_frame_still_delivers_one_and_marks_it() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();

    // Two 300-byte frames, each rendering as 899 hex + 300 text = 1199 bytes —
    // far beyond the minimum 512-byte budget. Forward progress requires the
    // first frame to be delivered anyway, alone and marked.
    let payload = format!("{}\n", "y".repeat(300)).repeat(2);
    tools::call("send", json!({ "session_id": sid, "text": payload }), &ctx).await.unwrap();

    let page = poll_budget_until_next_seq(&ctx, &sub, 2, 512).await;
    assert_eq!(frame_seqs(&page), vec![0], "an oversized frame is delivered alone");
    assert_eq!(page["oversized_frame"], json!(true));
    assert_eq!(page["page_bytes"], json!(1199));

    // Ack it: the second oversized frame gets its own marked page.
    let next = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 1, "max_inline_bytes": 512 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(frame_seqs(&next), vec![1]);
    assert_eq!(next["oversized_frame"], json!(true));
    assert_eq!(next["page_bytes"], json!(1199));
}

#[tokio::test]
async fn stream_poll_watermark_advances_only_with_inline_delivery() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();

    // Four 100-byte frames (399 metered bytes each); a 512-byte budget caps
    // every page at one frame, so only seq 0 has ever been delivered even
    // though all four were drained (next_seq = 4).
    let payload = format!("{}\n", "x".repeat(100)).repeat(4);
    tools::call("send", json!({ "session_id": sid, "text": payload }), &ctx).await.unwrap();
    let page = poll_budget_until_next_seq(&ctx, &sub, 4, 512).await;
    assert_eq!(frame_seqs(&page), vec![0]);
    assert_eq!(page["next_seq"], json!(4));

    // Acking past the byte-budget-capped delivery watermark is loud: frames
    // the budget held back were never delivered, so they must not be released.
    let err = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 4 }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("never delivered"), "got: {err:#}");

    // The correct ack — last delivered seq + 1 — releases only that frame, and
    // the held-back frames are still redelivered in order.
    let rest = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "since_seq": 1, "max_inline_bytes": 512 }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(frame_seqs(&rest), vec![1], "held-back frames survive the ack");
}

#[tokio::test]
async fn stream_poll_max_inline_bytes_range_is_loud() {
    let dir = scaffold_workspace();
    let ctx = ctx_for(&dir);
    let sid = open_echo_lines(&ctx).await;
    let created = tools::call("stream_poll", json!({ "session_id": sid }), &ctx).await.unwrap();
    let sub = created["subscription_id"].as_str().unwrap().to_string();

    for bad in [0u64, 511, 262_145, 4_294_967_301] {
        let err = tools::call(
            "stream_poll",
            json!({ "subscription_id": sub, "max_inline_bytes": bad }),
            &ctx,
        )
        .await
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("512..=262144") && msg.contains(&bad.to_string()),
            "max_inline_bytes {bad} must be rejected with the range and the value, got: {msg}"
        );
    }
    let err = tools::call(
        "stream_poll",
        json!({ "subscription_id": sub, "max_inline_bytes": "lots" }),
        &ctx,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("max_inline_bytes"), "got: {err:#}");
}
