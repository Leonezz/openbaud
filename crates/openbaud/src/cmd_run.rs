//! `openbaud run <device>/<command>` — execute a sedimented command without
//! any agent. This is the standalone proof that workspace knowledge is real,
//! and the entry point for CI regression against hardware.

use openbaud::engine::audit::Audit;
use openbaud::engine::session::Session;
use openbaud::engine::transport::open_port;
use openbaud::workspace::Workspace;
use anyhow::{anyhow, bail};
use openbaud_core::exec::{build_frame, parse_response, units};
use openbaud_core::format::Risk;
use openbaud_core::framing::Framing;
use openbaud_core::hex;
use serde_json::{json, Map, Value};
use std::path::Path;

pub async fn run(
    spec: &str,
    port: Option<&str>,
    sets: &[String],
    workspace_dir: &Path,
    acknowledge_risk: bool,
) -> anyhow::Result<()> {
    let (device_name, command_name) = spec
        .split_once('/')
        .ok_or_else(|| anyhow!("expected <device>/<command>, got {spec:?}"))?;
    let workspace = Workspace::at(workspace_dir);
    let audit = Audit::new(&workspace.root)?;
    let device = workspace.load_device(device_name)?;
    let cmd = device.command(command_name)?;

    if cmd.risk == Risk::Danger && !acknowledge_risk {
        audit.record(json!({
            "tool": "cli.run",
            "device": device_name,
            "command": command_name,
            "risk": "danger",
            "denied": true,
            "ok": false,
            "detail": "acknowledge_risk not set",
        }))?;
        bail!("command {spec} is marked risk=danger; rerun with --acknowledge-risk if you are sure");
    }
    let port = port.ok_or_else(|| anyhow!("--port is required (see `openbaud ports`)"))?;
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

    let tx = build_frame(cmd, &params)?;
    let framing = match &device.profile.framing {
        Some(spec) => spec.to_framing(&format!("devices/{device_name}/profile.yaml"))?,
        None => Framing::Idle { idle_ms: 30 },
    };
    let boxed = open_port(port, &device.profile.transport).await?;
    let session = Session::spawn("cli".to_string(), port.to_string(), framing, boxed);

    let outcome: anyhow::Result<Value> = async {
        if let Some(resp) = &cmd.response {
            let rule = resp.match_spec.to_rule(spec)?;
            let raw = session.request(&tx, rule, resp.timeout_ms).await?;
            let parsed = parse_response(cmd, &raw)?;
            Ok(json!({
                "parsed": parsed,
                "units": units(cmd),
                "tx_hex": hex::to_hex(&tx),
                "raw_hex": hex::to_hex(&raw),
            }))
        } else {
            session.write_raw(&tx).await?;
            Ok(json!({ "tx_hex": hex::to_hex(&tx), "note": "no response spec; frame sent" }))
        }
    }
    .await;

    audit.record(json!({
        "tool": "cli.run",
        "device": device_name,
        "command": command_name,
        "risk": format!("{:?}", cmd.risk).to_lowercase(),
        "port": port,
        "tx_hex": hex::to_hex(&tx),
        "ok": outcome.is_ok(),
        "detail": outcome.as_ref().err().map(|e| format!("{e:#}")),
    }))?;

    let result = outcome?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
