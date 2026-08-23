//! Result shaping: oversized result JSONs are written in full to
//! `.openbaud/out/` and returned as a deterministic summary view carrying a
//! `full_result` path — the agent's context never fills up with a huge
//! payload, and nothing is silently lost (every truncation is marked and the
//! complete file is always referenced).

use anyhow::Context;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// Default inline budget shared by the MCP tools and the CLI.
pub const DEFAULT_MAX_INLINE_BYTES: usize = 4096;

/// Strings longer than this many characters are truncated in the summary.
const STRING_INLINE_CHARS: usize = 256;
/// Arrays up to this many elements stay complete in the summary.
const ARRAY_INLINE_FULL: usize = 16;
/// Longer arrays keep this many leading elements plus a truncation marker.
const ARRAY_SUMMARY_HEAD: usize = 8;

/// Shape a result JSON for inline return. Results whose pretty serialization
/// fits in `max_inline_bytes` pass through untouched; larger ones are written
/// in full (pretty) to `<root>/.openbaud/out/res-<ms>-<tool>.json` and
/// replaced by a summary view: every object key survives, long strings are
/// truncated to a prefix plus an explicit byte count, long arrays keep their
/// head plus a `{"truncated": N}` marker, and the top level gains
/// `"full_result": "<workspace-relative path>"`. The summary is returned even
/// if it still exceeds the budget — the full-result pointer is never dropped.
pub fn shape_result(
    result: Value,
    workspace_root: &Path,
    tool: &str,
    max_inline_bytes: usize,
) -> anyhow::Result<Value> {
    let pretty =
        serde_json::to_string_pretty(&result).context("cannot serialize result JSON")?;
    if pretty.len() <= max_inline_bytes {
        return Ok(result);
    }

    let rel_path = write_full(&pretty, workspace_root, tool)?;
    let summary = summarize(&result);
    match summary {
        Value::Object(mut obj) => {
            obj.insert("full_result".to_string(), json!(rel_path));
            Ok(Value::Object(obj))
        }
        // Every tool result is an object today; if a non-object ever gets
        // here, wrap it rather than lose the full-result pointer.
        other => Ok(json!({ "summary": other, "full_result": rel_path })),
    }
}

/// Write the complete pretty JSON under `.openbaud/out/`, never overwriting:
/// a same-millisecond name collision appends a counter. Returns the path
/// relative to the workspace root.
fn write_full(pretty: &str, workspace_root: &Path, tool: &str) -> anyhow::Result<String> {
    let out_dir = workspace_root.join(".openbaud").join("out");
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("cannot create {}", out_dir.display()))?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before the UNIX epoch")?
        .as_millis();

    let mut counter: u32 = 0;
    loop {
        let name = match counter {
            0 => format!("res-{now_ms}-{tool}.json"),
            n => format!("res-{now_ms}-{tool}-{n}.json"),
        };
        let path = out_dir.join(&name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(pretty.as_bytes())
                    .and_then(|()| file.write_all(b"\n"))
                    .with_context(|| format!("cannot write {}", path.display()))?;
                return Ok(format!(".openbaud/out/{name}"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => counter += 1,
            Err(e) => {
                return Err(e).with_context(|| format!("cannot create {}", path.display()))
            }
        }
    }
}

/// The deterministic summary view, applied recursively to nested structures.
fn summarize(value: &Value) -> Value {
    match value {
        Value::String(s) => summarize_string(s),
        Value::Array(items) if items.len() > ARRAY_INLINE_FULL => {
            let mut out: Vec<Value> =
                items[..ARRAY_SUMMARY_HEAD].iter().map(summarize).collect();
            out.push(json!({ "truncated": items.len() - ARRAY_SUMMARY_HEAD }));
            Value::Array(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(summarize).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), summarize(v))).collect())
        }
        other => other.clone(),
    }
}

fn summarize_string(s: &str) -> Value {
    // Counting characters (not bytes) keeps the cut on a char boundary.
    let mut chars = s.char_indices();
    match chars.nth(STRING_INLINE_CHARS) {
        None => Value::String(s.to_string()),
        Some((cut, _)) => {
            Value::String(format!("{}…({} bytes total)", &s[..cut], s.len()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_results_pass_through_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let result = json!({ "outcome": "normal", "parsed": { "v": 1.5 } });
        let shaped = shape_result(result.clone(), dir.path(), "t", 4096).unwrap();
        assert_eq!(shaped, result);
        assert!(!dir.path().join(".openbaud/out").exists(), "nothing may be written");
    }

    #[test]
    fn oversized_result_is_spilled_and_summarized() {
        let dir = tempfile::tempdir().unwrap();
        let big_string = "x".repeat(600);
        let big_array: Vec<Value> = (0..100).map(Value::from).collect();
        let result = json!({ "s": big_string, "a": big_array, "n": 7 });

        let shaped = shape_result(result.clone(), dir.path(), "t", 64).unwrap();
        let s = shaped["s"].as_str().unwrap();
        assert!(s.starts_with("xxxx"));
        assert!(s.ends_with("…(600 bytes total)"), "got: {s}");
        let a = shaped["a"].as_array().unwrap();
        assert_eq!(a.len(), 9);
        assert_eq!(a[8], json!({ "truncated": 92 }));
        assert_eq!(shaped["n"], json!(7), "scalars survive untouched");

        let rel = shaped["full_result"].as_str().unwrap();
        let full: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(rel)).unwrap())
                .unwrap();
        assert_eq!(full, result, "the spilled file holds the complete result");
    }

    #[test]
    fn same_millisecond_collisions_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..3 {
            let shaped =
                shape_result(json!({ "i": i, "pad": "y".repeat(300) }), dir.path(), "t", 8)
                    .unwrap();
            paths.push(shaped["full_result"].as_str().unwrap().to_string());
        }
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 3, "each call must land in its own file");
        for (i, rel) in paths.iter().enumerate() {
            let full: Value =
                serde_json::from_str(&std::fs::read_to_string(dir.path().join(rel)).unwrap())
                    .unwrap();
            assert!(full["i"].is_number(), "file {i} holds a complete result");
        }
    }
}
