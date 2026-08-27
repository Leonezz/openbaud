//! MCP Apps UI templates (io.modelcontextprotocol/ui): single-file HTML built
//! from widgets/ and embedded at compile time. Hosts that render Apps fetch
//! these via resources/read; hosts that don't simply ignore the tool `_meta`
//! bindings and keep the text results — zero regression by construction.

use crate::output::SPILL_DIR;
use anyhow::{bail, Context};
use serde_json::{json, Value};
use std::path::Path;

/// Exact mimeType the extension requires for app templates.
pub const MIME: &str = "text/html;profile=mcp-app";

/// Saved tool results, served to the widget so full payloads reach the view
/// without passing through the model's context.
pub const RESULT_URI_PREFIX: &str = "openbaud://result/";

pub const PORT_PICKER_URI: &str = "ui://openbaud/port-picker.html";
pub const VIEWER_URI: &str = "ui://openbaud/viewer.html";

const PORT_PICKER_HTML: &str = include_str!("ui/port-picker.html");
const VIEWER_HTML: &str = include_str!("ui/viewer.html");

struct Template {
    uri: &'static str,
    name: &'static str,
    description: &'static str,
    html: &'static str,
}

const TEMPLATES: [Template; 2] = [
    Template {
        uri: PORT_PICKER_URI,
        name: "Port picker",
        description: "Candidate serial ports with one-click open (binds to list_ports).",
        html: PORT_PICKER_HTML,
    },
    Template {
        uri: VIEWER_URI,
        name: "Result viewer",
        description: "Schema-dispatching result viewer: polar radar plot for angular scans, \
                      key/value card for everything else (binds to run_command/request/read).",
        html: VIEWER_HTML,
    },
];

pub fn list() -> Value {
    let resources: Vec<Value> = TEMPLATES
        .iter()
        .map(|t| {
            json!({
                "uri": t.uri,
                "name": t.name,
                "description": t.description,
                "mimeType": MIME,
            })
        })
        .collect();
    json!({ "resources": resources })
}

/// Read a resource. `Ok(None)` means the uri is not ours; `Err` means it is
/// ours but unusable (missing file, or a name trying to escape the spill dir).
pub fn read(uri: &str, workspace_root: &Path) -> anyhow::Result<Option<Value>> {
    if let Some(name) = uri.strip_prefix(RESULT_URI_PREFIX) {
        return read_result(name, workspace_root).map(Some);
    }
    Ok(read_template(uri))
}

fn read_result(name: &str, workspace_root: &Path) -> anyhow::Result<Value> {
    if name.is_empty() || name.contains('/') || name.contains("..") {
        bail!("{name:?} is not a file directly inside {SPILL_DIR}/");
    }
    let path = workspace_root.join(SPILL_DIR).join(name);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {SPILL_DIR}/{name}"))?;
    Ok(json!({
        "contents": [{
            "uri": format!("{RESULT_URI_PREFIX}{name}"),
            "mimeType": "application/json",
            "text": text,
        }]
    }))
}

fn read_template(uri: &str) -> Option<Value> {
    let t = TEMPLATES.iter().find(|t| t.uri == uri)?;
    // No _meta.ui.csp: the extension's restrictive default already fits a
    // fully-inlined single file (inline script/style allowed, all network
    // blocked). ui.domain is deliberately absent — local stdio servers have
    // no URL to hash.
    Some(json!({
        "contents": [{
            "uri": t.uri,
            "mimeType": MIME,
            "text": t.html,
        }]
    }))
}
