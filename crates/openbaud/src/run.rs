//! Shared command/workflow executor behind both the MCP tools and the CLI.
//!
//! Executes a declared command on an open session and produces the classified
//! result JSON of the v0.1 slice (outcome / expect / expect_met / …). Does no
//! auditing — callers own the audit trail; the returned JSON carries
//! everything an audit entry needs.

use crate::engine::session::Session;
use crate::engine::transport::{self, REPLAY_PREFIX};
use crate::workspace::Device;
use anyhow::bail;
use openbaud_core::exec::{build_frame, classify, expect_met, units};
use openbaud_core::format::{Command, Risk, StepSpec, Workflow};
use openbaud_core::framing::MatchRule;
use openbaud_core::hex;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::time::Duration;

/// Execute one command on the session: apply `timing`, send the frame, await
/// and classify the response. Returns the result JSON plus whether the
/// command's declared expectation was met (`true` for commands without a
/// `response` block — sending is their success). `Err` is reserved for
/// infrastructure/caller failures (bad params, dead port); wire outcomes such
/// as silence or timeout are results, not errors.
pub async fn execute_command(
    session: &Session,
    device: &Device,
    cmd: &Command,
    params: &Map<String, Value>,
) -> anyhow::Result<(Value, bool)> {
    let tx = build_frame(cmd, params)?;
    if let Some(t) = &cmd.timing {
        if t.pre_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(t.pre_delay_ms)).await;
        }
    }

    let mut out = Map::new();
    out.insert("device".to_string(), json!(device.name));
    out.insert("command".to_string(), json!(cmd.name));
    out.insert("tx_hex".to_string(), json!(hex::to_hex(&tx)));

    let met = match &cmd.response {
        None => {
            session.write_raw(&tx).await?;
            out.insert("outcome".to_string(), json!("sent"));
            out.insert(
                "note".to_string(),
                json!("command declares no response spec; frame sent"),
            );
            true
        }
        Some(resp) => {
            let rule = match &resp.match_spec {
                Some(m) => m.to_rule(&format!("{}/{}", device.name, cmd.name))?,
                // `expect: silence` commands may omit `match`: no frame is
                // ever supposed to complete. Length(usize::MAX) is explicitly
                // unsatisfiable, so the engine waits out timeout_ms (or
                // first_byte_ms) and classifies Silence/Timeout on whatever
                // arrived.
                None => MatchRule::Length(usize::MAX),
            };
            let raw = session
                .request_classified(
                    &tx,
                    rule,
                    resp.exception.as_ref(),
                    resp.timeout_ms,
                    resp.first_byte_ms,
                )
                .await?;
            let classified = classify(cmd, raw);
            let met = expect_met(cmd, &classified.outcome);

            out.insert("outcome".to_string(), serde_json::to_value(classified.outcome)?);
            out.insert("expect".to_string(), serde_json::to_value(resp.expect)?);
            out.insert("expect_met".to_string(), json!(met));
            if let Some(raw) = &classified.raw {
                out.insert("raw_hex".to_string(), json!(hex::to_hex(raw)));
                out.insert("raw_text".to_string(), json!(hex::to_text_lossy(raw)));
            }
            if let Some(parsed) = classified.parsed {
                out.insert("parsed".to_string(), parsed);
            }
            if let Some(exception) = classified.exception {
                out.insert("exception".to_string(), exception);
            }
            if let Some(partial) = &classified.partial {
                out.insert("partial_hex".to_string(), json!(hex::to_hex(partial)));
            }
            if let Some(detail) = classified.detail {
                out.insert("detail".to_string(), json!(detail));
            }
            // Only present when a checksum was actually declared and passed —
            // a `normal` outcome alone never means anything was verified.
            if let Some(algorithm) = classified.checksum_verified {
                out.insert("checksum".to_string(), json!(algorithm));
            }
            let units = units(cmd);
            if !units.is_empty() {
                out.insert("units".to_string(), Value::Object(units));
            }
            // The command's declared encoding travels with the result so a
            // viewer knows which field feeds which channel — it never guesses.
            if let Some(view) = cmd.response.as_ref().and_then(|r| r.view.as_ref()) {
                out.insert("view".to_string(), serde_json::to_value(view)?);
            }
            met
        }
    };

    if let Some(t) = &cmd.timing {
        if t.post_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(t.post_delay_ms)).await;
        }
        // Record the timing that actually applied, per the design.
        out.insert(
            "timing".to_string(),
            json!({ "pre_delay_ms": t.pre_delay_ms, "post_delay_ms": t.post_delay_ms }),
        );
    }
    Ok((Value::Object(out), met))
}

/// Execute a workflow on the session: steps in order, first failure skips the
/// remaining steps; `finally` steps are all attempted regardless, failures
/// recorded per step. Result JSON:
/// `{ok, device, workflow, steps: [...], finally: [...], skipped: [...]}` —
/// every step entry carries `ok` plus the full command result JSON (or an
/// `error` when the step could not execute at all).
pub async fn execute_workflow(
    session: &Session,
    device: &Device,
    wf: &Workflow,
) -> anyhow::Result<(Value, bool)> {
    let mut steps_out: Vec<Value> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut failed = false;

    for step in &wf.steps {
        if failed {
            skipped.push(step.command.clone());
            continue;
        }
        // A dead session is the only error worth aborting the workflow for;
        // anything else is this step's failure.
        let entry = run_step(session, device, step, true).await?;
        if entry.get("ok") != Some(&json!(true)) {
            failed = true;
        }
        steps_out.push(entry);
    }

    let mut finally_out: Vec<Value> = Vec::new();
    let mut finally_ok = true;
    for step in &wf.finally {
        // `finally` steps are attempted unconditionally, one failure never
        // blocks the next — even a dead session is recorded, not thrown.
        let entry = run_step(session, device, step, false).await?;
        if entry.get("ok") != Some(&json!(true)) {
            finally_ok = false;
        }
        finally_out.push(entry);
    }

    let ok = !failed && finally_ok;
    let result = json!({
        "ok": ok,
        "device": device.name,
        "workflow": wf.name,
        "steps": steps_out,
        "finally": finally_out,
        "skipped": skipped,
    });
    Ok((result, ok))
}

/// Run one workflow step, folding execution errors into the step entry.
/// With `propagate_fatal`, a session-level fatal error is returned as `Err`
/// instead (used for `steps`; `finally` records everything).
async fn run_step(
    session: &Session,
    device: &Device,
    step: &StepSpec,
    propagate_fatal: bool,
) -> anyhow::Result<Value> {
    let outcome = async {
        let cmd = device.command(&step.command)?;
        let params = step.params.clone().unwrap_or_default();
        execute_command(session, device, cmd, &params).await
    }
    .await;
    match outcome {
        Ok((mut entry, ok)) => {
            entry
                .as_object_mut()
                .expect("execute_command results are objects")
                .insert("ok".to_string(), json!(ok));
            Ok(entry)
        }
        Err(e) if propagate_fatal && is_session_fatal(&e) => Err(e),
        Err(e) => Ok(json!({
            "command": step.command,
            "ok": false,
            "error": format!("{e:#}"),
        })),
    }
}

/// Whether an execution error means the session itself is dead. The engine
/// marks every dead-port path with "session port failed: …" (the message
/// `Session::check_alive` produces once the reader loop records a transport
/// failure); other errors — bad params, frame build, spec issues — are
/// per-step failures.
fn is_session_fatal(err: &anyhow::Error) -> bool {
    format!("{err:#}").contains("session port failed")
}

/// The workflow's risk is the maximum over its referenced commands. Returns
/// the maximum plus the names of the `danger` commands (for acknowledgement
/// error messages). Errors if a referenced command does not exist — impossible
/// for workflows that passed workspace loading.
pub fn workflow_risk(device: &Device, wf: &Workflow) -> anyhow::Result<(Risk, Vec<String>)> {
    let rank = |r: Risk| match r {
        Risk::Read => 0,
        Risk::Write => 1,
        Risk::Danger => 2,
    };
    let mut max = Risk::Read;
    let mut danger: Vec<String> = Vec::new();
    for name in wf.referenced_commands() {
        let cmd = device.command(&name)?;
        if rank(cmd.risk) > rank(max) {
            max = cmd.risk;
        }
        if cmd.risk == Risk::Danger {
            danger.push(name);
        }
    }
    Ok((max, danger))
}

/// Normalize a caller-provided port string: a `replay:` port with a relative
/// capture path is resolved against the workspace root, everything else
/// passes through untouched.
pub fn resolve_port_arg(port: &str, workspace_root: &Path) -> String {
    if let Some(rest) = port.strip_prefix(REPLAY_PREFIX) {
        let path = Path::new(rest);
        if path.is_relative() {
            return format!("{REPLAY_PREFIX}{}", workspace_root.join(path).display());
        }
    }
    port.to_string()
}

/// Resolve the port to open for a device: an explicit port wins (with
/// `replay:` paths resolved against the workspace); without one, the device's
/// profile selector is consulted — exactly one live match required. No port
/// and no selector is a loud error naming both routes.
pub fn resolve_port(
    port: Option<&str>,
    device: &Device,
    workspace_root: &Path,
) -> anyhow::Result<String> {
    match port {
        Some(p) => Ok(resolve_port_arg(p, workspace_root)),
        None => match &device.profile.selector {
            Some(selector) => transport::resolve_selector(selector, &device.name),
            None => bail!(
                "no port given and device {:?} declares no selector — pass a port explicitly \
                 (see list_ports), or add `selector: {{vid, pid, serial_number, product}}` to \
                 devices/{}/profile.yaml for automatic resolution",
                device.name,
                device.name
            ),
        },
    }
}
