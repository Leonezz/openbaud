//! Pure protocol logic for openbaud. No IO in this crate.

pub mod checksum;
pub mod codec;
pub mod exec;
pub mod format;
pub mod framing;
pub mod hex;
pub mod schema;
pub mod template;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid hex string {input:?}: {reason}")]
    InvalidHex { input: String, reason: String },
    #[error(
        "unknown checksum {0:?} (expected crc16_modbus, crc16_ccitt, crc8, crc32, xor8, sum8 or sum16be)"
    )]
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
    #[error("checksum mismatch at byte {at}: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String, at: usize },
}

pub type Result<T> = std::result::Result<T, CoreError>;
