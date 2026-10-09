//! `openbaud run <device>/<name>` — execute a sedimented command or workflow
//! without any agent. This is the standalone proof that workspace knowledge is
//! real, and the entry point for CI regression against hardware.

use anyhow::{anyhow, bail};
use openbaud::engine::audit::Audit;
use openbaud::engine::facts::Facts;
use openbaud::preconditions;
use openbaud::engine::session::Session;
use openbaud::engine::transport::open_port;
use openbaud::output::shape_result;
use openbaud::run::{ensure_sendable, execute_command, execute_workflow, resolve_port, workflow_risk};
use openbaud::workspace::{Device, Workspace};
use openbaud_core::format::Risk;
use openbaud_core::framing::Framing;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::Arc;

/// The CLI's execution switches.
pub struct RunOptions {
    /// Required to execute commands marked risk=danger.
    pub acknowledge_risk: bool,
    /// Run although `requires` facts are not established on the session.
    pub assume_preconditions: bool,
    pub max_inline_bytes: usize,
}

pub async fn run(
    spec: &str,
    port: Option<&str>,
    sets: &[String],
    workspace_dir: &Path,
    opts: &RunOptions,
) -> anyhow::Result<()> {
    let (device_name, name) = spec
        .split_once('/')
        .ok_or_else(|| anyhow!("expected <device>/<command-or-workflow>, got {spec:?}"))?;
    let workspace = Workspace::at(workspace_dir);
    let audit = Audit::new(&workspace.root)?;
    let device = workspace.load_device(device_name)?;

    // Commands shadow workflows by construction (name conflicts are rejected
    // at load time), so command lookup goes first.
    if device.commands.contains_key(name) {
        return run_command_cli(
            spec, &device, name, port, sets, &workspace, &audit, opts,
        )
        .await;
    }
    if device.workflows.contains_key(name) {
        return run_workflow_cli(
            spec, &device, name, port, sets, &workspace, &audit, opts,
        )
        .await;
    }

    let mut commands: Vec<&str> = device.commands.keys().map(String::as_str).collect();
    let mut workflows: Vec<&str> = device.workflows.keys().map(String::as_str).collect();
    commands.sort();
    workflows.sort();
    let mut msg = format!(
        "device {device_name:?} has no command or workflow {name:?} (commands: [{}]; workflows: [{}])",
        commands.join(", "),
        workflows.join(", ")
    );
    for warning in device.broken_warnings() {
        msg.push_str(&format!("; {warning}"));
    }
    bail!(msg);
}

async fn open_session(
    device: &Device,
    port: Option<&str>,
    workspace: &Workspace,
) -> anyhow::Result<Arc<Session>> {
    let port = resolve_port(port, device, &workspace.root)?;
    let framing = match &device.profile.framing {
        Some(spec) => spec.to_framing(&format!("devices/{}/profile.yaml", device.name))?,
        None => Framing::Idle { idle_ms: 30 },
    };
    let boxed = open_port(&port, &device.profile.transport, Some(&framing)).await?;
    Ok(Session::spawn("cli".to_string(), port, framing, boxed))
}

#[allow(clippy::too_many_arguments)]
async fn run_command_cli(
    spec: &str,
    device: &Device,
    name: &str,
    port: Option<&str>,
    sets: &[String],
    workspace: &Workspace,
    audit: &Audit,
    opts: &RunOptions,
) -> anyhow::Result<()> {
    let cmd = device.command(name)?;
    if let Err(e) = ensure_sendable(&device.name, cmd) {
        audit.record(json!({
            "tool": "cli.run",
            "device": device.name,
            "command": name,
            "ok": false,
            "detail": format!("{e:#}"),
        }))?;
        return Err(e);
    }

    if cmd.risk == Risk::Danger && !opts.acknowledge_risk {
        audit.record(json!({
            "tool": "cli.run",
            "device": device.name,
            "command": name,
            "risk": "danger",
            "denied": true,
            "ok": false,
            "detail": "acknowledge_risk not set",
        }))?;
        bail!("command {spec} is marked risk=danger; rerun with --acknowledge-risk if you are sure");
    }
    for warning in device.broken_warnings() {
        eprintln!("warning: {warning}");
    }

    let mut params: Map<String, Value> = Map::new();
    for set in sets {
        let (key, raw) = set
            .split_once('=')
            .ok_or_else(|| anyhow!("--set expects key=value, got {set:?}"))?;
        let value = serde_json::from_str(raw).unwrap_or_else(|_| Value::from(raw));
        params.insert(key.to_string(), value);
    }

    // A CLI session is always fresh — it knows no facts — so an unmet
    // `requires` is certain and refused before the port is opened.
    if let Err(e) = preconditions::check(device, cmd, &Facts::default(), opts.assume_preconditions) {
        audit.record(json!({
            "tool": "cli.run",
            "device": device.name,
            "command": name,
            "ok": false,
            "detail": format!("{e:#}"),
        }))?;
        return Err(e);
    }

    let session = open_session(device, port, workspace).await?;
    let exec = execute_command(&session, device, cmd, &params, opts.assume_preconditions).await;

    let mut entry = json!({
        "tool": "cli.run",
        "device": device.name,
        "command": name,
        "risk": format!("{:?}", cmd.risk).to_lowercase(),
        "port": session.port_name,
    });
    let obj = entry.as_object_mut().expect("object");
    match &exec {
        Ok((result, ok)) => {
            for key in ["tx_hex", "outcome"] {
                if let Some(v) = result.get(key) {
                    obj.insert(key.to_string(), v.clone());
                }
            }
            // "verified" / "assumed": a run on assumed state stays on the record.
            if let Some(status) = result
                .get("preconditions")
                .and_then(Value::as_object)
                .and_then(|o| o.keys().next())
            {
                obj.insert("preconditions".to_string(), json!(status));
            }
            obj.insert("ok".to_string(), json!(*ok));
            if !ok {
                obj.insert("detail".to_string(), json!("expectation not met"));
            }
        }
        Err(e) => {
            obj.insert("ok".to_string(), json!(false));
            obj.insert("detail".to_string(), json!(format!("{e:#}")));
        }
    }
    audit.record(entry)?;

    let (result, ok) = exec?;
    let result = shape_result(result, &workspace.root, "cli.run", opts.max_inline_bytes)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    if !ok {
        bail!(
            "command {spec} expectation not met — outcome {}, expected {}",
            result.get("outcome").cloned().unwrap_or_default(),
            result.get("expect").cloned().unwrap_or_default(),
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_workflow_cli(
    spec: &str,
    device: &Device,
    name: &str,
    port: Option<&str>,
    sets: &[String],
    workspace: &Workspace,
    audit: &Audit,
    opts: &RunOptions,
) -> anyhow::Result<()> {
    if !sets.is_empty() {
        bail!("--set does not apply to workflows — parameters live in the workflow's steps");
    }
    let wf = device.workflow(name)?;
    let (risk, danger_steps) = workflow_risk(device, wf)?;

    if risk == Risk::Danger && !opts.acknowledge_risk {
        audit.record(json!({
            "tool": "cli.run",
            "device": device.name,
            "workflow": name,
            "risk": "danger",
            "denied": true,
            "ok": false,
            "detail": "acknowledge_risk not set",
        }))?;
        bail!(
            "workflow {spec} contains risk=danger command(s): [{}]; rerun with --acknowledge-risk if you are sure",
            danger_steps.join(", ")
        );
    }
    for warning in device.broken_warnings() {
        eprintln!("warning: {warning}");
    }

    if let Err(e) =
        preconditions::simulate_workflow(device, wf, &Facts::default(), opts.assume_preconditions)
    {
        audit.record(json!({
            "tool": "cli.run",
            "device": device.name,
            "workflow": name,
            "ok": false,
            "detail": format!("{e:#}"),
        }))?;
        return Err(e);
    }

    let session = open_session(device, port, workspace).await?;
    let exec = execute_workflow(&session, device, wf, opts.assume_preconditions).await;

    let (result, ok) = match exec {
        Ok(ok) => ok,
        Err(e) => {
            audit.record(json!({
                "tool": "cli.run",
                "device": device.name,
                "workflow": name,
                "risk": format!("{risk:?}").to_lowercase(),
                "port": session.port_name,
                "ok": false,
                "detail": format!("{e:#}"),
            }))?;
            return Err(e);
        }
    };

    // One audit entry per executed step, then the workflow-level entry — same
    // shape as the MCP run_workflow tool.
    for (phase, key) in [("step", "steps"), ("finally", "finally")] {
        for step in result[key].as_array().into_iter().flatten() {
            let mut step_entry = json!({
                "tool": "cli.run.step",
                "device": device.name,
                "workflow": name,
                "phase": phase,
                "port": session.port_name,
            });
            let obj = step_entry.as_object_mut().expect("object");
            for field in ["command", "ok", "outcome", "tx_hex", "detail", "error"] {
                if let Some(v) = step.get(field) {
                    obj.insert(field.to_string(), v.clone());
                }
            }
            audit.record(step_entry)?;
        }
    }
    audit.record(json!({
        "tool": "cli.run",
        "device": device.name,
        "workflow": name,
        "risk": format!("{risk:?}").to_lowercase(),
        "port": session.port_name,
        "ok": ok,
        "detail": if ok { Value::Null } else { json!("workflow failed (see cli.run.step entries)") },
    }))?;

    // Shaping happens after the per-step audit extraction above — audit
    // entries always see the untruncated result.
    let result = shape_result(result, &workspace.root, "cli.run", opts.max_inline_bytes)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    if !ok {
        bail!("workflow {spec} failed");
    }
    Ok(())
}
