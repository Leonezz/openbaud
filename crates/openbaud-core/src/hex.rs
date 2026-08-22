//! Hex string helpers. Canonical display form is uppercase pairs separated by
//! single spaces ("01 04 CA"); parsing accepts any whitespace grouping as long
//! as every group has even length.

use crate::CoreError;

pub fn parse_hex(input: &str) -> crate::Result<Vec<u8>> {
    let mut out = Vec::new();
    for group in input.split_whitespace() {
        if group.len() % 2 != 0 {
            return Err(CoreError::InvalidHex {
                input: input.to_string(),
                reason: format!("group {group:?} has odd length"),
            });
        }
        for i in (0..group.len()).step_by(2) {
            let pair = &group[i..i + 2];
            let byte = u8::from_str_radix(pair, 16).map_err(|_| CoreError::InvalidHex {
                input: input.to_string(),
                reason: format!("{pair:?} is not a hex byte"),
            })?;
            out.push(byte);
        }
    }
    Ok(out)
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lossy printable rendering of raw bytes for agent consumption: printable
/// ASCII kept, everything else shown as `\xNN`.
pub fn to_text_lossy(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_grouping() {
        assert_eq!(parse_hex("01 04 a5B4").unwrap(), vec![0x01, 0x04, 0xA5, 0xB4]);
        assert_eq!(parse_hex("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn rejects_odd_and_junk() {
        assert!(parse_hex("0 1").is_err());
        assert!(parse_hex("zz").is_err());
    }

    #[test]
    fn round_trip() {
        let bytes = vec![0x00, 0xFF, 0x31];
        assert_eq!(parse_hex(&to_hex(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn lossy_text() {
        assert_eq!(to_text_lossy(b"OK\r\n\x01"), "OK\\r\\n\\x01");
    }
}
