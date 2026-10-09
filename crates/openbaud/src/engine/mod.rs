pub mod audit;
pub mod capture;
pub mod replay;
pub mod session;
pub mod stream;
pub mod transport;

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// This server process's identity in the workspace's shared records.
/// Session ids restart at s1 in every process, and several processes (agent
/// hosts) can share one workspace at the same time; the audit log and every
/// capture header carry this id so a capture's events are told apart from a
/// same-named session of another process.
pub fn run_id() -> &'static str {
    static RUN_ID: OnceLock<String> = OnceLock::new();
    RUN_ID.get_or_init(|| format!("{}@{}", std::process::id(), now_ms()))
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as u64
}
