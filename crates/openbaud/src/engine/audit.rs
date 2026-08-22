//! Append-only audit log for every write-capable operation, successful or not.
//! Failure to audit is a failure of the operation — never silent.

use anyhow::Context;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Audit {
    path: PathBuf,
}

impl Audit {
    pub fn new(workspace_root: &Path) -> anyhow::Result<Self> {
        let dir = workspace_root.join(".openbaud");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create audit directory {dir:?}"))?;
        Ok(Self { path: dir.join("audit.jsonl") })
    }

    pub fn record(&self, mut entry: Value) -> anyhow::Result<()> {
        let obj = entry
            .as_object_mut()
            .context("audit entry must be a JSON object")?;
        obj.insert("ts_ms".to_string(), Value::from(crate::engine::now_ms()));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("cannot open audit log {:?}", self.path))?;
        writeln!(file, "{entry}")
            .with_context(|| format!("cannot append to audit log {:?}", self.path))?;
        Ok(())
    }
}
