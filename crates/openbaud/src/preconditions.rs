//! Command preconditions: a command's `requires` checked against the facts
//! its session established, before a byte is sent.
//!
//! A frame can be well-formed, checksummed and parse cleanly while meaning
//! something else entirely — a Modbus read against the wrong meter profile
//! returns a valid frame whose "voltage" is another register. Preconditions
//! turn the state a command depends on from prose into a check: an unmet or
//! unknown fact refuses the command with what is wrong and how to fix it.
//! Assuming them is possible, but only as an explicit, recorded choice.

use crate::engine::facts::{FactState, Facts};
use crate::workspace::Device;
use anyhow::{anyhow, bail};
use openbaud_core::exec::resolve_params;
use openbaud_core::format::{Command, Workflow};
use openbaud_core::template::{render_text, text_placeholders};
use serde_json::{json, Map, Value};

/// Check `cmd.requires` against `facts`. `Ok(None)`: the command requires
/// nothing. `Ok(Some(report))`: `{verified: {...}}`, or `{assumed: {...}}`
/// when unmet facts were explicitly assumed. `Err`: an unmet fact, with the
/// reason and the commands that can establish it.
pub fn check(
    device: &Device,
    cmd: &Command,
    facts: &Facts,
    assume: bool,
) -> anyhow::Result<Option<Value>> {
    if cmd.requires.is_empty() {
        return Ok(None);
    }
    let required: Map<String, Value> = cmd
        .requires
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    let unmet: Vec<String> = cmd
        .requires
        .iter()
        .filter_map(|(key, want)| unmet_reason(device, key, want, facts))
        .collect();
    if unmet.is_empty() {
        return Ok(Some(json!({ "verified": required })));
    }
    if assume {
        return Ok(Some(json!({ "assumed": required })));
    }
    bail!(
        "command {}/{} refused — its preconditions are not met on this session: {}. \
         If you know the device is already in that state, retry with \
         assume_preconditions: true (CLI: --assume-preconditions); the result and the \
         audit log then record the preconditions as assumed",
        device.name,
        cmd.name,
        unmet.join("; ")
    )
}

/// Why `key` is not `want` on this session, or `None` when it is.
fn unmet_reason(device: &Device, key: &str, want: &str, facts: &Facts) -> Option<String> {
    let how = establishers(device, key, want);
    let fix = if how.is_empty() {
        format!("no command of {} establishes {key} = {want}", device.name)
    } else {
        format!(
            "establish it first on this session with {}",
            how.join(" or ")
        )
    };
    match facts.get(key) {
        FactState::Known(fact) if fact.value.eq_ignore_ascii_case(want) => None,
        FactState::Known(fact) => Some(format!(
            "it requires {key} = {want}, but {key} is {} (established by {}) — {fix}",
            fact.value, fact.by
        )),
        FactState::Lost(reason) => Some(format!(
            "it requires {key} = {want}, but {key} is unknown: {reason} — {fix}"
        )),
        FactState::Never => Some(format!(
            "it requires {key} = {want}, which nothing on this session has established — {fix}"
        )),
    }
}

/// The commands of this device that can establish `key = want`: those whose
/// value is a template over their params (the caller picks the value), or a
/// literal equal to `want`. A command that only ever establishes some other
/// literal is no way to reach `want` and is never suggested.
fn establishers(device: &Device, key: &str, want: &str) -> Vec<String> {
    let mut names: Vec<String> = device
        .commands
        .values()
        .filter(|c| {
            c.establishes.get(key).is_some_and(|template| {
                let templated = text_placeholders(template).is_ok_and(|names| !names.is_empty());
                templated || template.eq_ignore_ascii_case(want)
            })
        })
        .map(|c| format!("{}/{}", device.name, c.name))
        .collect();
    names.sort();
    names
}

/// The facts `cmd` establishes for these params, rendered before anything is
/// sent so a bad value fails without touching the wire.
pub fn render_establishes(
    cmd: &Command,
    params: &Map<String, Value>,
) -> anyhow::Result<Vec<(String, String)>> {
    if cmd.establishes.is_empty() {
        return Ok(Vec::new());
    }
    let values = resolve_params(cmd, params)?;
    cmd.establishes
        .iter()
        .map(|(key, template)| {
            let value = render_text(template, &values)
                .map_err(|e| anyhow!("{}: establishes.{key}: {e}", cmd.name))?;
            Ok((key.clone(), value))
        })
        .collect()
}

/// Record the outcome of an establishing command on `facts`: on success
/// (`failure: None`) the facts it establishes; on any failure every fact it
/// would have set is lost — the device may or may not have changed.
pub fn record_outcome(
    facts: &mut Facts,
    device: &Device,
    cmd: &Command,
    established: &[(String, String)],
    failure: Option<&str>,
) {
    let by = format!("{}/{}", device.name, cmd.name);
    match failure {
        None => {
            for (key, value) in established {
                facts.establish(key, value, &by);
            }
        }
        Some(what) => {
            for key in cmd.establishes.keys() {
                facts.lose(
                    key,
                    &format!("{by} {what}, so the device may or may not have changed"),
                );
            }
        }
    }
}

/// Walk a workflow's steps and `finally` from `start`, checking each step's
/// preconditions and applying what it establishes as if it succeeded — so a
/// workflow that can never satisfy one of its steps is refused before its
/// session is opened and before any byte is sent.
pub fn simulate_workflow(
    device: &Device,
    wf: &Workflow,
    start: &Facts,
    assume: bool,
) -> anyhow::Result<()> {
    let mut facts = start.clone();
    for (phase, steps) in [("step", &wf.steps), ("finally step", &wf.finally)] {
        for (i, step) in steps.iter().enumerate() {
            let cmd = device.command(&step.command)?;
            check(device, cmd, &facts, assume).map_err(|e| {
                anyhow!(
                    "workflow {}/{} {phase} {} ({}): {e:#}",
                    device.name,
                    wf.name,
                    i + 1,
                    step.command
                )
            })?;
            let params = step.params.clone().unwrap_or_default();
            let established = render_establishes(cmd, &params)?;
            record_outcome(&mut facts, device, cmd, &established, None);
        }
    }
    Ok(())
}
