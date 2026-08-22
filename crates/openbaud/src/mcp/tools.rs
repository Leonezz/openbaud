//! The nine MCP tools: schemas and dispatch.

use crate::engine::transport::{self, open_port};
use crate::mcp::Ctx;
use anyhow::{anyhow, bail, Context as _};
use openbaud_core::exec::{build_frame, parse_response, units};
use openbaud_core::format::{FramingSpec, MatchSpec, Risk};
use openbaud_core::framing::{Framing, MatchRule};
use openbaud_core::hex;
use serde_json::{json, Map, Value};
use std::sync::Arc;

pub const SERVER_INSTRUCTIONS: &str = "openbaud gives you structured, audited access to serial \
ports plus a workspace knowledge format. Device knowledge lives in devices/<name>/ \
(profile.yaml, commands/*.yaml, notes.md) in the current workspace — read those files directly, \
and sediment what you learn about a device into them. Port 'mock:echo' always exists for \
smoke-testing without hardware. Every write is appended to .openbaud/audit.jsonl. Prefer \
run_command (typed, parsed, provenance-tracked) over raw send/request once a command exists.";

/// Default framing when neither profile nor caller specifies one: frame on a
/// 30 ms receive gap, a sane exploration default.
fn default_framing() -> Framing {
    Framing::Idle { idle_ms: 30 }
}

pub fn list() -> Vec<Value> {
    let ro = json!({ "readOnlyHint": true });
    vec![
        json!({
            "name": "list_ports",
            "description": "List serial ports (USB metadata included) plus the always-available mock:echo loopback.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": ro,
        }),
        json!({
            "name": "open",
            "description": "Open a serial port session. If `device` names a workspace device, its profile supplies transport defaults and framing; explicit arguments override. Returns a session_id.",
            "inputSchema": { "type": "object", "required": ["port"], "properties": {
                "port": { "type": "string", "description": "Port path from list_ports, or mock:echo" },
                "device": { "type": "string", "description": "Workspace device name supplying defaults" },
                "baud": { "type": "integer" },
                "data_bits": { "type": "integer", "minimum": 5, "maximum": 8 },
                "parity": { "type": "string", "enum": ["none", "even", "odd"] },
                "stop_bits": { "type": "integer", "minimum": 1, "maximum": 2 },
                "framing": { "type": "object", "description": "One of {delimiter}|{idle_ms}|{length_prefix:{header_len,len_at,len_size,endian,extra}}" }
            }},
        }),
        json!({
            "name": "close",
            "description": "Close a session and release the port.",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" }
            }},
        }),
        json!({
            "name": "read",
            "description": "Cursor-based framed read: returns frames received since your previous read (waits up to timeout_ms for the first frame). Reports dropped_bytes if the buffer overflowed.",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" },
                "timeout_ms": { "type": "integer", "default": 500 },
                "max_frames": { "type": "integer", "default": 32 }
            }},
            "annotations": ro,
        }),
        json!({
            "name": "send",
            "description": "Write raw bytes (audited). Provide exactly one of hex ('01 A0 FF') or text (control chars as-is, e.g. \"AT\\r\\n\").",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" },
                "hex": { "type": "string" },
                "text": { "type": "string" }
            }},
        }),
        json!({
            "name": "request",
            "description": "Send bytes and await the response (audited). `match` decides when the response is complete: {length:N} | {delimiter:\"...\"} | {idle_ms:N}; default is a 50 ms idle gap.",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" },
                "hex": { "type": "string" },
                "text": { "type": "string" },
                "match": { "type": "object" },
                "timeout_ms": { "type": "integer", "default": 1000 }
            }},
        }),
        json!({
            "name": "run_command",
            "description": "Execute a named command from devices/<device>/commands/ with typed params: builds the frame, awaits and parses the response per the command spec (audited). Give session_id for an open session, or port to open an ephemeral one with the device's profile transport. Commands with risk=danger additionally require acknowledge_risk=true.",
            "inputSchema": { "type": "object", "required": ["device", "command"], "properties": {
                "device": { "type": "string" },
                "command": { "type": "string" },
                "params": { "type": "object" },
                "session_id": { "type": "string" },
                "port": { "type": "string" },
                "acknowledge_risk": { "type": "boolean" }
            }},
        }),
        json!({
            "name": "capture_start",
            "description": "Start lossless timestamped capture of all rx/tx on a session into captures/*.obcap (JSONL).",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" },
                "note": { "type": "string" }
            }},
            "annotations": ro,
        }),
        json!({
            "name": "capture_stop",
            "description": "Stop the active capture on a session; returns path and stats.",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" }
            }},
            "annotations": ro,
        }),
    ]
}

pub async fn call(name: &str, args: Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    match name {
        "list_ports" => Ok(json!({ "ports": transport::list_ports()? })),
        "open" => tool_open(args, ctx).await,
        "close" => {
            let id = arg_str(&args, "session_id")?;
            ctx.sessions.close(id)?;
            Ok(json!({ "closed": id }))
        }
        "read" => {
            let session = ctx.sessions.get(arg_str(&args, "session_id")?)?;
            let timeout = arg_u64_or(&args, "timeout_ms", 500)?;
            let max = arg_u64_or(&args, "max_frames", 32)? as usize;
            let result = session.read_frames(timeout, max.max(1)).await?;
            Ok(serde_json::to_value(result)?)
        }
        "send" => tool_send(args, ctx).await,
        "request" => tool_request(args, ctx).await,
        "run_command" => tool_run_command(args, ctx).await,
        "capture_start" => {
            let session = ctx.sessions.get(arg_str(&args, "session_id")?)?;
            let note = args.get("note").and_then(Value::as_str);
            let path = ctx.workspace.capture_path(&session.id);
            let path = session.capture_start(&path, note)?;
            Ok(json!({ "path": path }))
        }
        "capture_stop" => {
            let session = ctx.sessions.get(arg_str(&args, "session_id")?)?;
            Ok(serde_json::to_value(session.capture_stop()?)?)
        }
        other => bail!("unknown tool {other:?}"),
    }
}

async fn tool_open(args: Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let port = arg_str(&args, "port")?;
    let device = match args.get("device").and_then(Value::as_str) {
        Some(name) => Some(ctx.workspace.load_device(name)?),
        None => None,
    };
    let mut cfg = device.as_ref().map(|d| d.profile.transport.clone()).unwrap_or_default();
    if let Some(b) = args.get("baud").and_then(Value::as_u64) {
        cfg.baud = u32::try_from(b).context("baud out of range")?;
    }
    if let Some(b) = args.get("data_bits").and_then(Value::as_u64) {
        cfg.data_bits = b as u8;
    }
    if let Some(b) = args.get("stop_bits").and_then(Value::as_u64) {
        cfg.stop_bits = b as u8;
    }
    if let Some(p) = args.get("parity").and_then(Value::as_str) {
        cfg.parity = serde_yaml::from_str(p)
            .map_err(|_| anyhow!("parity must be none, even or odd, got {p:?}"))?;
    }
    let framing = resolve_framing(args.get("framing"), device.as_ref().map(|d| &d.profile))?;

    let boxed = open_port(port, &cfg)?;
    let session = ctx.sessions.open(port, framing.clone(), boxed);
    Ok(json!({
        "session_id": session.id,
        "port": port,
        "baud": cfg.baud,
        "framing": format!("{framing:?}"),
    }))
}

fn resolve_framing(
    override_spec: Option<&Value>,
    profile: Option<&openbaud_core::format::Profile>,
) -> anyhow::Result<Framing> {
    if let Some(spec) = override_spec {
        let spec: FramingSpec = serde_json::from_value(spec.clone())
            .map_err(|e| anyhow!("invalid framing spec: {e}"))?;
        return Ok(spec.to_framing("open.framing")?);
    }
    if let Some(profile) = profile {
        if let Some(spec) = &profile.framing {
            return Ok(spec.to_framing(&format!("devices/{}/profile.yaml", profile.name))?);
        }
    }
    Ok(default_framing())
}

fn payload_bytes(args: &Value) -> anyhow::Result<Vec<u8>> {
    match (args.get("hex").and_then(Value::as_str), args.get("text").and_then(Value::as_str)) {
        (Some(h), None) => Ok(hex::parse_hex(h)?),
        (None, Some(t)) => Ok(t.as_bytes().to_vec()),
        _ => bail!("provide exactly one of: hex, text"),
    }
}

async fn tool_send(args: Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let session = ctx.sessions.get(arg_str(&args, "session_id")?)?;
    let data = payload_bytes(&args)?;
    let outcome = session.write_raw(&data).await;
    ctx.audit.record(json!({
        "tool": "send",
        "session": session.id,
        "port": session.port_name,
        "tx_hex": hex::to_hex(&data),
        "ok": outcome.is_ok(),
        "detail": outcome.as_ref().err().map(|e| format!("{e:#}")),
    }))?;
    let written = outcome?;
    Ok(json!({ "bytes_written": written }))
}

async fn tool_request(args: Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let session = ctx.sessions.get(arg_str(&args, "session_id")?)?;
    let data = payload_bytes(&args)?;
    let rule = match args.get("match") {
        Some(m) => {
            let spec: MatchSpec = serde_json::from_value(m.clone())
                .map_err(|e| anyhow!("invalid match spec: {e}"))?;
            spec.to_rule("request.match")?
        }
        None => MatchRule::Idle { idle_ms: 50 },
    };
    let timeout = arg_u64_or(&args, "timeout_ms", 1000)?;
    let outcome = session.request(&data, rule, timeout).await;
    ctx.audit.record(json!({
        "tool": "request",
        "session": session.id,
        "port": session.port_name,
        "tx_hex": hex::to_hex(&data),
        "ok": outcome.is_ok(),
        "detail": outcome.as_ref().err().map(|e| format!("{e:#}")),
    }))?;
    let raw = outcome?;
    Ok(json!({ "hex": hex::to_hex(&raw), "text": hex::to_text_lossy(&raw) }))
}

async fn tool_run_command(args: Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let device_name = arg_str(&args, "device")?;
    let command_name = arg_str(&args, "command")?;
    let device = ctx.workspace.load_device(device_name)?;
    let cmd = device.command(command_name)?;

    if cmd.risk == Risk::Danger && !args.get("acknowledge_risk").and_then(Value::as_bool).unwrap_or(false) {
        bail!(
            "command {device_name}/{command_name} is marked risk=danger{}. Confirm with the user, then retry with acknowledge_risk=true.",
            cmd.description.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()
        );
    }

    let params: Map<String, Value> = match args.get("params") {
        Some(Value::Object(m)) => m.clone(),
        Some(other) => bail!("params must be an object, got {other}"),
        None => Map::new(),
    };
    let tx = build_frame(cmd, &params)?;

    let (session, ephemeral) = match args.get("session_id").and_then(Value::as_str) {
        Some(id) => (ctx.sessions.get(id)?, false),
        None => {
            let port = args.get("port").and_then(Value::as_str).ok_or_else(|| {
                anyhow!("provide session_id (open session) or port (ephemeral session)")
            })?;
            let framing = resolve_framing(None, Some(&device.profile))?;
            let boxed = open_port(port, &device.profile.transport)?;
            (ctx.sessions.open(port, framing, boxed), true)
        }
    };

    let outcome: anyhow::Result<Value> = async {
        if let Some(resp) = &cmd.response {
            let rule = resp.match_spec.to_rule(&format!("{device_name}/{command_name}"))?;
            let raw = session.request(&tx, rule, resp.timeout_ms).await?;
            let parsed = parse_response(cmd, &raw)?;
            Ok(json!({
                "device": device_name,
                "command": command_name,
                "tx_hex": hex::to_hex(&tx),
                "raw_hex": hex::to_hex(&raw),
                "raw_text": hex::to_text_lossy(&raw),
                "parsed": parsed,
                "units": units(cmd),
            }))
        } else {
            session.write_raw(&tx).await?;
            Ok(json!({
                "device": device_name,
                "command": command_name,
                "tx_hex": hex::to_hex(&tx),
                "note": "command declares no response spec; frame sent",
            }))
        }
    }
    .await;

    if ephemeral {
        // The id was just created by us; failure here can only mean it is
        // already gone, which is fine.
        let _ = ctx.sessions.close(&session.id);
    }
    ctx.audit.record(json!({
        "tool": "run_command",
        "device": device_name,
        "command": command_name,
        "risk": format!("{:?}", cmd.risk).to_lowercase(),
        "session": session.id,
        "port": session.port_name,
        "tx_hex": hex::to_hex(&tx),
        "ok": outcome.is_ok(),
        "detail": outcome.as_ref().err().map(|e| format!("{e:#}")),
    }))?;
    outcome
}

fn arg_str<'a>(args: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing required string argument {key:?}"))
}

fn arg_u64_or(args: &Value, key: &str, default: u64) -> anyhow::Result<u64> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_u64().ok_or_else(|| anyhow!("argument {key:?} must be a non-negative integer")),
    }
}
