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
    assert_eq!(listed.len(), 11);
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
    for name in ["list_ports", "read", "schema"] {
        let tool = listed.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], json!(true), "tool {name}");
    }
    for name in ["list_ports", "open", "close", "read", "send", "request", "run_command", "run_workflow"] {
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

    // Replay via a workspace-relative capture path.
    let abs = std::path::PathBuf::from(started["path"].as_str().unwrap());
    let rel = abs.strip_prefix(dir.path()).expect("capture lives inside the workspace");
    let replayed = tools::call(
        "run_command",
        json!({
            "device": "echodev",
            "command": "ping",
            "port": format!("replay:{}", rel.display()),
        }),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(replayed["outcome"], json!("normal"));
    assert_eq!(replayed["expect_met"], json!(true));
    assert_eq!(replayed["parsed"]["y"], json!(66));
    assert_eq!(replayed["parsed"], live["parsed"], "replay must reproduce the live result");
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
