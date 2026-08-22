//! Lossless timestamped wire capture (.obcap): JSONL, one line per chunk,
//! header line first. Human- and script-readable by design.

use anyhow::Context;
use openbaud_core::hex;
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct CaptureWriter {
    file: File,
    path: PathBuf,
    frames: u64,
    bytes: u64,
}

#[derive(Debug, serde::Serialize)]
pub struct CaptureStats {
    pub path: String,
    pub frames: u64,
    pub bytes: u64,
}

impl CaptureWriter {
    pub fn create(
        path: &Path,
        session: &str,
        port: &str,
        note: Option<&str>,
        started_ms: u64,
    ) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create capture directory {parent:?}"))?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .with_context(|| format!("cannot create capture file {path:?}"))?;
        let header = json!({
            "obcap": 1,
            "session": session,
            "port": port,
            "note": note,
            "started_ms": started_ms,
        });
        writeln!(file, "{header}").context("cannot write capture header")?;
        Ok(Self { file, path: path.to_path_buf(), frames: 0, bytes: 0 })
    }

    pub fn record(&mut self, dir: &str, ts_ms: u64, data: &[u8]) -> anyhow::Result<()> {
        let line = json!({ "ts_ms": ts_ms, "dir": dir, "hex": hex::to_hex(data) });
        writeln!(self.file, "{line}")
            .with_context(|| format!("cannot append to capture file {:?}", self.path))?;
        self.frames += 1;
        self.bytes += data.len() as u64;
        Ok(())
    }

    pub fn finish(mut self) -> anyhow::Result<CaptureStats> {
        self.file.flush().context("cannot flush capture file")?;
        Ok(CaptureStats {
            path: self.path.display().to_string(),
            frames: self.frames,
            bytes: self.bytes,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
