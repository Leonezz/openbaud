//! The eleven MCP tools: schemas and dispatch.

use crate::engine::session::Session;
use crate::engine::transport::{self, open_port, resolve_selector};
use crate::mcp::Ctx;
use crate::output::{self, shape_result};
use crate::run;
use crate::workspace::Device;
use anyhow::{anyhow, bail};
use openbaud_core::format::{FramingSpec, MatchSpec, Risk};
use openbaud_core::framing::{Framing, MatchRule};
use openbaud_core::hex;
use openbaud_core::schema::{example, json_schema, SchemaKind};
use serde_json::{json, Map, Value};
use std::sync::Arc;

pub const SERVER_INSTRUCTIONS: &str = "openbaud gives you structured, audited access to serial \
ports plus a workspace knowledge format. Device knowledge lives in devices/<name>/ \
(profile.yaml, commands/*.yaml, workflows/*.yaml, notes.md) in the current workspace — read \
those files directly, and sediment what you learn about a device into them. Use the schema \
tool to get the full JSON Schema and an annotated example of each format before writing or \
editing YAML under devices/. Port 'mock:echo' always exists for smoke-testing without \
hardware; 'replay:<capture path>' replays a recorded .obcap. Every write is appended to \
.openbaud/audit.jsonl. Prefer run_command (typed, parsed, provenance-tracked) over raw \
send/request once a command exists.";

/// Default framing when neither profile nor caller specifies one: frame on a
/// 30 ms receive gap, a sane exploration default.
fn default_framing() -> Framing {
    Framing::Idle { idle_ms: 30 }
}

/// Input-schema fragment for the shared `max_inline_bytes` tool parameter.
fn max_inline_bytes_schema() -> Value {
    json!({
        "type": "integer",
        "default": output::DEFAULT_MAX_INLINE_BYTES,
        "description": "Result JSONs longer than this many bytes are written in full to \
            .openbaud/out/ and returned as a summary carrying a full_result path — keeps huge \
            payloads out of your context. Raise it to force large results inline."
    })
}

fn arg_max_inline(args: &Value) -> anyhow::Result<usize> {
    Ok(arg_u64_or(args, "max_inline_bytes", output::DEFAULT_MAX_INLINE_BYTES as u64)? as usize)
}

pub fn list() -> Vec<Value> {
    // Codex uses all three MCP behavior hints when deciding how prominently to
    // surface a tool and whether an invocation needs extra user attention.
    // A serial device is an external entity, so hardware I/O is open-world even
    // though OpenBaud itself does not use the network.
    let read_only_local = json!({
        "readOnlyHint": true,
        "openWorldHint": false,
        "destructiveHint": false
    });
    let read_only_hardware = json!({
        "readOnlyHint": true,
        "openWorldHint": true,
        "destructiveHint": false
    });
    let local_mutation = json!({
        "readOnlyHint": false,
        "openWorldHint": false,
        "destructiveHint": false
    });
    let hardware_session = json!({
        "readOnlyHint": false,
        "openWorldHint": true,
        "destructiveHint": false
    });
    let hardware_write = json!({
        "readOnlyHint": false,
        "openWorldHint": true,
        "destructiveHint": true
    });
    vec![
        json!({
            "name": "list_ports",
            "description": "List serial ports (USB metadata included) plus the always-available mock:echo loopback.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": read_only_hardware,
        }),
        json!({
            "name": "open",
            "description": "Open a serial port session. If `device` names a workspace device, its profile supplies transport defaults and framing; explicit arguments override. `port` may be omitted when the device profile declares a `selector` (vid/pid/serial_number/product) — exactly one live match is required. `replay:<capture path>` (relative to the workspace) replays a recorded .obcap. Returns a session_id.",
            "inputSchema": { "type": "object", "properties": {
                "port": { "type": "string", "description": "Port path from list_ports, mock:echo, or replay:<capture>. Optional when the device profile has a selector" },
                "device": { "type": "string", "description": "Workspace device name supplying defaults" },
                "baud": { "type": "integer" },
                "data_bits": { "type": "integer", "minimum": 5, "maximum": 8 },
                "parity": { "type": "string", "enum": ["none", "even", "odd"] },
                "stop_bits": { "type": "integer", "minimum": 1, "maximum": 2 },
                "framing": { "type": "object", "description": "One of {delimiter}|{idle_ms}|{length_prefix:{header_len,len_at,len_size,endian,extra}}" }
            }},
            "annotations": hardware_session,
        }),
        json!({
            "name": "close",
            "description": "Close a session and release the port.",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" }
            }},
            "annotations": hardware_session,
        }),
        json!({
            "name": "read",
            "description": "Cursor-based framed read: returns frames received since your previous read (waits up to timeout_ms for the first frame). Reports dropped_bytes if the buffer overflowed.",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" },
                "timeout_ms": { "type": "integer", "default": 500 },
                "max_frames": { "type": "integer", "default": 32 },
                "max_inline_bytes": max_inline_bytes_schema()
            }},
            "annotations": read_only_hardware,
        }),
        json!({
            "name": "send",
            "description": "Write raw bytes (audited). Provide exactly one of hex ('01 A0 FF') or text (control chars as-is, e.g. \"AT\\r\\n\").",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" },
                "hex": { "type": "string" },
                "text": { "type": "string" }
            }},
            "annotations": hardware_write,
        }),
        json!({
            "name": "request",
            "description": "Send bytes and await the response (audited). `match` decides when the response is complete: {length:N} | {delimiter:\"...\"} | {idle_ms:N}; default is a 50 ms idle gap.",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" },
                "hex": { "type": "string" },
                "text": { "type": "string" },
                "match": { "type": "object" },
                "timeout_ms": { "type": "integer", "default": 3000 },
                "max_inline_bytes": max_inline_bytes_schema()
            }},
            "annotations": hardware_write,
        }),
        json!({
            "name": "run_command",
            "description": "Execute a named command from devices/<device>/commands/ with typed params: builds the frame, awaits the response and classifies the outcome (normal | exception | silence | timeout | checksum_error | malformed) against the command's declared `expect` (audited; an unmet expectation is an error carrying the full result JSON). Give session_id for an open session, or port to open an ephemeral one with the device's profile transport — port may be omitted when the profile declares a selector, and accepts replay:<capture path>. Commands with risk=danger additionally require acknowledge_risk=true.",
            "inputSchema": { "type": "object", "required": ["device", "command"], "properties": {
                "device": { "type": "string" },
                "command": { "type": "string" },
                "params": { "type": "object" },
                "session_id": { "type": "string" },
                "port": { "type": "string", "description": "Optional when session_id is given or the device profile has a selector" },
                "acknowledge_risk": { "type": "boolean" },
                "max_inline_bytes": max_inline_bytes_schema()
            }},
            "annotations": hardware_write,
        }),
        json!({
            "name": "run_workflow",
            "description": "Execute a workflow from devices/<device>/workflows/: its steps run in order on one session (step params override command defaults); the first failing step skips the rest, then every `finally` step is attempted regardless. Returns {ok, steps, finally, skipped}; ok=false is an error carrying the full result JSON. Workflow risk is the maximum of its commands' risk — danger requires acknowledge_risk=true. Session resolution is the same as run_command (session_id, port, or profile selector).",
            "inputSchema": { "type": "object", "required": ["device", "workflow"], "properties": {
                "device": { "type": "string" },
                "workflow": { "type": "string" },
                "session_id": { "type": "string" },
                "port": { "type": "string", "description": "Optional when session_id is given or the device profile has a selector" },
                "acknowledge_risk": { "type": "boolean" },
                "max_inline_bytes": max_inline_bytes_schema()
            }},
            "annotations": hardware_write,
        }),
        json!({
            "name": "schema",
            "description": "The authoritative JSON Schema (or, with example=true, an annotated YAML example) of the openbaud knowledge formats — profile, command, workflow. Call this before writing or modifying any YAML under devices/: it is generated from the exact types that parse those files and includes the semantic rules the schema grammar cannot express.",
            "inputSchema": { "type": "object", "required": ["kind"], "properties": {
                "kind": { "type": "string", "enum": ["profile", "command", "workflow"] },
                "example": { "type": "boolean", "default": false, "description": "Return an annotated YAML example instead of the JSON Schema" }
            }},
            "annotations": read_only_local,
        }),
        json!({
            "name": "capture_start",
            "description": "Start lossless timestamped capture of all rx/tx on a session into captures/*.obcap (JSONL).",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" },
                "note": { "type": "string" }
            }},
            "annotations": local_mutation,
        }),
        json!({
            "name": "capture_stop",
            "description": "Stop the active capture on a session; returns path and stats.",
            "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                "session_id": { "type": "string" }
            }},
            "annotations": local_mutation,
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
            let max_inline = arg_max_inline(&args)?;
            let result = session.read_frames(timeout, max.max(1)).await?;
            shape_result(serde_json::to_value(result)?, &ctx.workspace.root, "read", max_inline)
        }
        "send" => tool_send(args, ctx).await,
        "request" => tool_request(args, ctx).await,
        "run_command" => tool_run_command(args, ctx).await,
        "run_workflow" => tool_run_workflow(args, ctx).await,
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
        "schema" => tool_schema(&args),
        other => bail!("unknown tool {other:?}"),
    }
}

async fn tool_open(args: Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let device = match args.get("device").and_then(Value::as_str) {
        Some(name) => Some(ctx.workspace.load_device(name)?),
        None => None,
    };
    let port = match args.get("port").and_then(Value::as_str) {
        Some(p) => run::resolve_port_arg(p, &ctx.workspace.root),
        None => match device.as_ref().and_then(|d| d.profile.selector.as_ref()) {
            Some(selector) => {
                resolve_selector(selector, &device.as_ref().expect("selector implies device").name)?
            }
            None => bail!(
                "no port given — pass a port explicitly (see list_ports), or give a device \
                 whose profile declares a `selector` for automatic resolution"
            ),
        },
    };
    let port = port.as_str();
    let mut cfg = device.as_ref().map(|d| d.profile.transport.clone()).unwrap_or_default();
    if let Some(v) = args.get("baud") {
        let b = v
            .as_u64()
            .filter(|b| *b > 0)
            .ok_or_else(|| anyhow!("baud must be a positive integer, got {v}"))?;
        cfg.baud = u32::try_from(b).map_err(|_| anyhow!("baud {b} out of range"))?;
    }
    if let Some(v) = args.get("data_bits") {
        let b = v
            .as_u64()
            .filter(|b| (5..=8).contains(b))
            .ok_or_else(|| anyhow!("data_bits must be an integer in 5..=8, got {v}"))?;
        cfg.data_bits = b as u8;
    }
    if let Some(v) = args.get("stop_bits") {
        let b = v
            .as_u64()
            .filter(|b| (1..=2).contains(b))
            .ok_or_else(|| anyhow!("stop_bits must be 1 or 2, got {v}"))?;
        cfg.stop_bits = b as u8;
    }
    if let Some(v) = args.get("parity") {
        let p = v.as_str().ok_or_else(|| anyhow!("parity must be a string, got {v}"))?;
        cfg.parity = serde_yaml::from_str(p)
            .map_err(|_| anyhow!("parity must be none, even or odd, got {p:?}"))?;
    }
    let framing = resolve_framing(args.get("framing"), device.as_ref().map(|d| &d.profile))?;

    let boxed = open_port(port, &cfg).await?;
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

fn tool_schema(args: &Value) -> anyhow::Result<Value> {
    let kind_str = arg_str(args, "kind")?;
    let kind = match kind_str {
        "profile" => SchemaKind::Profile,
        "command" => SchemaKind::Command,
        "workflow" => SchemaKind::Workflow,
        other => bail!("unknown schema kind {other:?} (expected profile, command or workflow)"),
    };
    Ok(if args.get("example").and_then(Value::as_bool).unwrap_or(false) {
        json!({ "kind": kind_str, "example": example(kind) })
    } else {
        json!({ "kind": kind_str, "schema": json_schema(kind) })
    })
}

fn payload_bytes(args: &Value) -> anyhow::Result<Vec<u8>> {
    match (args.get("hex").and_then(Value::as_str), args.get("text").and_then(Value::as_str)) {
        (Some(h), None) => Ok(hex::parse_hex(h)?),
        (None, Some(t)) => Ok(t.as_bytes().to_vec()),
        _ => bail!("provide exactly one of: hex, text"),
    }
}

/// Append a failed-attempt entry to the audit log. Write-capable operations
/// leave a trace on every path, including failures before any byte is sent.
fn audit_fail(ctx: &Ctx, mut base: Value, err: &anyhow::Error) -> anyhow::Result<()> {
    let obj = base.as_object_mut().expect("audit base is always a JSON object");
    obj.insert("ok".to_string(), json!(false));
    obj.insert("detail".to_string(), json!(format!("{err:#}")));
    ctx.audit.record(base)
}

async fn tool_send(args: Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let sid = arg_str(&args, "session_id")?.to_string();
    let staged = ctx
        .sessions
        .get(&sid)
        .and_then(|session| Ok((session, payload_bytes(&args)?)));
    let (session, data) = match staged {
        Ok(ok) => ok,
        Err(e) => {
            audit_fail(ctx, json!({ "tool": "send", "session": sid }), &e)?;
            return Err(e);
        }
    };
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
    let sid = arg_str(&args, "session_id")?.to_string();
    let staged = (|| {
        let session = ctx.sessions.get(&sid)?;
        let data = payload_bytes(&args)?;
        let rule = match args.get("match") {
            Some(m) => {
                let spec: MatchSpec = serde_json::from_value(m.clone())
                    .map_err(|e| anyhow!("invalid match spec: {e}"))?;
                spec.to_rule("request.match")?
            }
            None => MatchRule::Idle { idle_ms: 50 },
        };
        let timeout = arg_u64_or(&args, "timeout_ms", 3000)?;
        let max_inline = arg_max_inline(&args)?;
        Ok((session, data, rule, timeout, max_inline))
    })();
    let (session, data, rule, timeout, max_inline) = match staged {
        Ok(ok) => ok,
        Err(e) => {
            audit_fail(ctx, json!({ "tool": "request", "session": sid }), &e)?;
            return Err(e);
        }
    };
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
    let result = json!({ "hex": hex::to_hex(&raw), "text": hex::to_text_lossy(&raw) });
    shape_result(result, &ctx.workspace.root, "request", max_inline)
}

/// Session for run_command/run_workflow: an existing one by id, or an
/// ephemeral one over `port` — resolved via the device's profile selector
/// when no port is given. Returns (session, is_ephemeral).
async fn resolve_session(
    args: &Value,
    device: &Device,
    ctx: &Arc<Ctx>,
) -> anyhow::Result<(Arc<Session>, bool)> {
    match args.get("session_id").and_then(Value::as_str) {
        Some(id) => Ok((ctx.sessions.get(id)?, false)),
        None => {
            let port = run::resolve_port(
                args.get("port").and_then(Value::as_str),
                device,
                &ctx.workspace.root,
            )?;
            let framing = resolve_framing(None, Some(&device.profile))?;
            let boxed = open_port(&port, &device.profile.transport).await?;
            Ok((ctx.sessions.open(&port, framing, boxed), true))
        }
    }
}

/// Attach the device's broken-file warnings to a successful result.
fn attach_warnings(result: &mut Value, device: &Device) {
    if !device.broken.is_empty() {
        result
            .as_object_mut()
            .expect("run results are objects")
            .insert("warnings".to_string(), json!(device.broken_warnings()));
    }
}

async fn tool_run_command(args: Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let device_name = arg_str(&args, "device")?.to_string();
    let command_name = arg_str(&args, "command")?.to_string();
    let base = json!({ "tool": "run_command", "device": device_name, "command": command_name });

    let device = match ctx.workspace.load_device(&device_name) {
        Ok(d) => d,
        Err(e) => {
            audit_fail(ctx, base, &e)?;
            return Err(e);
        }
    };
    let cmd = match device.command(&command_name) {
        Ok(c) => c.clone(),
        Err(e) => {
            audit_fail(ctx, base, &e)?;
            return Err(e);
        }
    };
    let risk = format!("{:?}", cmd.risk).to_lowercase();
    let mut base = base;
    base.as_object_mut().expect("object").insert("risk".to_string(), json!(risk));

    if cmd.risk == Risk::Danger && !args.get("acknowledge_risk").and_then(Value::as_bool).unwrap_or(false) {
        let e = anyhow!(
            "command {device_name}/{command_name} is marked risk=danger{}. Confirm with the user, then retry with acknowledge_risk=true.",
            cmd.description.as_deref().map(|d| format!(" ({d})")).unwrap_or_default()
        );
        let mut denied = base.clone();
        denied.as_object_mut().expect("object").insert("denied".to_string(), json!(true));
        audit_fail(ctx, denied, &e)?;
        return Err(e);
    }

    let staged = (|| {
        let params = match args.get("params") {
            Some(Value::Object(m)) => m.clone(),
            Some(other) => bail!("params must be an object, got {other}"),
            None => Map::new(),
        };
        Ok((params, arg_max_inline(&args)?))
    })();
    let (params, max_inline) = match staged {
        Ok(p) => p,
        Err(e) => {
            audit_fail(ctx, base, &e)?;
            return Err(e);
        }
    };

    let (session, ephemeral) = match resolve_session(&args, &device, ctx).await {
        Ok(ok) => ok,
        Err(e) => {
            audit_fail(ctx, base, &e)?;
            return Err(e);
        }
    };

    let exec = run::execute_command(&session, &device, &cmd, &params).await;
    if ephemeral {
        // The id was just created by us; failure here can only mean it is
        // already gone, which is fine.
        let _ = ctx.sessions.close(&session.id);
    }

    let entry = base.as_object_mut().expect("object");
    entry.insert("session".to_string(), json!(session.id));
    entry.insert("port".to_string(), json!(session.port_name));
    match &exec {
        Ok((result, ok)) => {
            for key in ["tx_hex", "outcome"] {
                if let Some(v) = result.get(key) {
                    entry.insert(key.to_string(), v.clone());
                }
            }
            entry.insert("ok".to_string(), json!(*ok));
            if !ok {
                entry.insert("detail".to_string(), json!("expectation not met"));
            }
        }
        Err(e) => {
            entry.insert("ok".to_string(), json!(false));
            entry.insert("detail".to_string(), json!(format!("{e:#}")));
        }
    }
    ctx.audit.record(base)?;

    let (mut result, ok) = exec?;
    attach_warnings(&mut result, &device);
    // Shaping happens after auditing — the audit trail always sees the
    // untruncated fields — and on both the success and the unmet-expectation
    // paths, so a huge payload never floods the caller's context.
    let result = shape_result(result, &ctx.workspace.root, "run_command", max_inline)?;
    if !ok {
        bail!(
            "command {device_name}/{command_name} expectation not met — outcome {}, expected {}. Full result:\n{}",
            result.get("outcome").cloned().unwrap_or_default(),
            result.get("expect").cloned().unwrap_or_default(),
            serde_json::to_string_pretty(&result).expect("result is valid JSON"),
        );
    }
    Ok(result)
}

async fn tool_run_workflow(args: Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let device_name = arg_str(&args, "device")?.to_string();
    let workflow_name = arg_str(&args, "workflow")?.to_string();
    let base = json!({ "tool": "run_workflow", "device": device_name, "workflow": workflow_name });

    let staged = (|| {
        let device = ctx.workspace.load_device(&device_name)?;
        let wf = device.workflow(&workflow_name)?.clone();
        let (risk, danger_steps) = run::workflow_risk(&device, &wf)?;
        Ok((device, wf, risk, danger_steps, arg_max_inline(&args)?))
    })();
    let (device, wf, risk, danger_steps, max_inline) = match staged {
        Ok(ok) => ok,
        Err(e) => {
            audit_fail(ctx, base, &e)?;
            return Err(e);
        }
    };
    let mut base = base;
    base.as_object_mut()
        .expect("object")
        .insert("risk".to_string(), json!(format!("{risk:?}").to_lowercase()));

    if risk == Risk::Danger && !args.get("acknowledge_risk").and_then(Value::as_bool).unwrap_or(false) {
        let e = anyhow!(
            "workflow {device_name}/{workflow_name} contains risk=danger command(s): [{}]. \
             Confirm with the user, then retry with acknowledge_risk=true.",
            danger_steps.join(", ")
        );
        let mut denied = base.clone();
        denied.as_object_mut().expect("object").insert("denied".to_string(), json!(true));
        audit_fail(ctx, denied, &e)?;
        return Err(e);
    }

    let (session, ephemeral) = match resolve_session(&args, &device, ctx).await {
        Ok(ok) => ok,
        Err(e) => {
            audit_fail(ctx, base, &e)?;
            return Err(e);
        }
    };

    let exec = run::execute_workflow(&session, &device, &wf).await;
    if ephemeral {
        let _ = ctx.sessions.close(&session.id);
    }

    let entry = base.as_object_mut().expect("object");
    entry.insert("session".to_string(), json!(session.id));
    entry.insert("port".to_string(), json!(session.port_name));
    let (mut result, ok) = match exec {
        Ok(ok) => ok,
        Err(e) => {
            audit_fail(ctx, base, &e)?;
            return Err(e);
        }
    };

    // One audit entry per executed step (steps and finally), then the
    // workflow-level entry.
    for (phase, key) in [("step", "steps"), ("finally", "finally")] {
        for step in result[key].as_array().into_iter().flatten() {
            let mut step_entry = json!({
                "tool": "run_workflow.step",
                "device": device_name,
                "workflow": workflow_name,
                "phase": phase,
                "session": session.id,
                "port": session.port_name,
            });
            let obj = step_entry.as_object_mut().expect("object");
            for field in ["command", "ok", "outcome", "tx_hex", "detail", "error"] {
                if let Some(v) = step.get(field) {
                    obj.insert(field.to_string(), v.clone());
                }
            }
            ctx.audit.record(step_entry)?;
        }
    }
    entry.insert("ok".to_string(), json!(ok));
    if !ok {
        entry.insert("detail".to_string(), json!("workflow failed (see run_workflow.step entries)"));
    }
    ctx.audit.record(base)?;

    attach_warnings(&mut result, &device);
    // Shaping happens after the per-step audit extraction above (audit always
    // sees the untruncated result) and applies to both the success and the
    // failed-workflow paths.
    let result = shape_result(result, &ctx.workspace.root, "run_workflow", max_inline)?;
    if !ok {
        bail!(
            "workflow {device_name}/{workflow_name} failed. Full result:\n{}",
            serde_json::to_string_pretty(&result).expect("result is valid JSON"),
        );
    }
    Ok(result)
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
