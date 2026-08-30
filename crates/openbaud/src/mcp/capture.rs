//! The capture pair: `capture_start` / `capture_stop`. Both report the
//! capture's workspace-relative path ("captures/<file>") — the exact shape
//! `capture_frames`, `session_timeline` and `replay:<path>` take verbatim —
//! alongside the absolute path.

use crate::mcp::tools::arg_str;
use crate::mcp::Ctx;
use anyhow::anyhow;
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) fn capture_start(args: &Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let session = ctx.sessions.get(arg_str(args, "session_id")?)?;
    let note = args.get("note").and_then(Value::as_str);
    let path = ctx.workspace.capture_path(&session.id);
    let abs = session.capture_start(&path, note)?;
    Ok(json!({ "path": rel_path(&abs)?, "abs_path": abs }))
}

pub(crate) fn capture_stop(args: &Value, ctx: &Arc<Ctx>) -> anyhow::Result<Value> {
    let session = ctx.sessions.get(arg_str(args, "session_id")?)?;
    let stats = session.capture_stop()?;
    let rel = rel_path(&stats.path)?;
    let mut out = serde_json::to_value(&stats)?;
    let obj = out.as_object_mut().expect("capture stats serialize to an object");
    obj.insert("abs_path".to_string(), json!(stats.path));
    obj.insert("path".to_string(), json!(rel));
    Ok(out)
}

/// Workspace-relative form ("captures/<file>") of an absolute capture path.
/// Every capture is created via `Workspace::capture_path`, so a path without
/// a file name is a bug, reported loudly.
fn rel_path(abs: &str) -> anyhow::Result<String> {
    let name = std::path::Path::new(abs)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("capture path {abs:?} carries no file name"))?;
    Ok(format!("captures/{name}"))
}
