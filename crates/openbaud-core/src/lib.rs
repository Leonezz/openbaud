//! Pure protocol logic for openbaud. No IO in this crate.

pub mod checksum;
pub mod codec;
pub mod exec;
pub mod format;
pub mod framing;
pub mod hex;
pub mod template;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid hex string {input:?}: {reason}")]
    InvalidHex { input: String, reason: String },
    #[error("unknown checksum {0:?} (expected crc16_modbus, xor8 or sum8)")]
    UnknownChecksum(String),
    #[error("unknown field type {0:?}")]
    UnknownFieldType(String),
    #[error("template error: {0}")]
    Template(String),
    #[error("parameter {name:?}: {reason}")]
    Param { name: String, reason: String },
    #[error("format error in {path}: {reason}")]
    Format { path: String, reason: String },
    #[error("response parse error: {0}")]
    Parse(String),
    #[error("checksum mismatch: expected {expected} at frame tail, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

pub type Result<T> = std::result::Result<T, CoreError>;
