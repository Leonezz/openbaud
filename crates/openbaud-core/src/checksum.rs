//! Checksum algorithms supported by the v0 command format. Each checksum is
//! appended to the frame tail; CRC16/MODBUS and CRC-32 are emitted
//! little-endian per their wire conventions (Modbus RTU, zlib), CRC-16/CCITT
//! big-endian.

use crate::{hex, CoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumKind {
    Crc16Modbus,
    /// CRC-16/CCITT-FALSE: poly 0x1021, init 0xFFFF, no reflection, emitted
    /// as two big-endian bytes.
    Crc16Ccitt,
    /// CRC-8 (SMBus): poly 0x07, init 0x00, one byte.
    Crc8,
    /// CRC-32 (IEEE, zlib): reflected, init 0xFFFFFFFF, final complement,
    /// emitted as four little-endian bytes.
    Crc32,
    Xor8,
    Sum8,
    /// 16-bit sum of all bytes, emitted as two big-endian bytes (PMS5003 style).
    Sum16Be,
}

impl ChecksumKind {
    pub fn from_name(name: &str) -> crate::Result<Self> {
        match name {
            "crc16_modbus" => Ok(Self::Crc16Modbus),
            "crc16_ccitt" => Ok(Self::Crc16Ccitt),
            "crc8" => Ok(Self::Crc8),
            "crc32" => Ok(Self::Crc32),
            "xor8" => Ok(Self::Xor8),
            "sum8" => Ok(Self::Sum8),
            "sum16be" => Ok(Self::Sum16Be),
            other => Err(CoreError::UnknownChecksum(other.to_string())),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Crc16Modbus => "crc16_modbus",
            Self::Crc16Ccitt => "crc16_ccitt",
            Self::Crc8 => "crc8",
            Self::Crc32 => "crc32",
            Self::Xor8 => "xor8",
            Self::Sum8 => "sum8",
            Self::Sum16Be => "sum16be",
        }
    }

    /// Number of bytes this checksum occupies at the frame tail.
    #[allow(clippy::len_without_is_empty)] // a checksum width is never zero
    pub fn len(&self) -> usize {
        match self {
            Self::Crc16Modbus | Self::Crc16Ccitt | Self::Sum16Be => 2,
            Self::Crc32 => 4,
            Self::Crc8 | Self::Xor8 | Self::Sum8 => 1,
        }
    }

    pub fn compute(&self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Crc16Modbus => crc16_modbus(data).to_le_bytes().to_vec(),
            Self::Crc16Ccitt => crc16_ccitt(data).to_be_bytes().to_vec(),
            Self::Crc8 => vec![crc8_smbus(data)],
            Self::Crc32 => crc32_ieee(data).to_le_bytes().to_vec(),
            Self::Xor8 => vec![data.iter().fold(0u8, |acc, b| acc ^ b)],
            Self::Sum8 => vec![data.iter().fold(0u8, |acc, b| acc.wrapping_add(*b))],
            Self::Sum16Be => data
                .iter()
                .fold(0u16, |acc, b| acc.wrapping_add(*b as u16))
                .to_be_bytes()
                .to_vec(),
        }
    }

    /// Verify a frame whose tail carries this checksum over all preceding bytes.
    pub fn verify_frame(&self, frame: &[u8]) -> crate::Result<()> {
        let n = self.len();
        if frame.len() < n + 1 {
            return Err(CoreError::Parse(format!(
                "frame of {} bytes is too short to carry a {} checksum",
                frame.len(),
                self.name()
            )));
        }
        let (body, tail) = frame.split_at(frame.len() - n);
        let expected = self.compute(body);
        if tail != expected {
            return Err(CoreError::ChecksumMismatch {
                expected: hex::to_hex(&expected),
                actual: hex::to_hex(tail),
                at: frame.len() - n,
            });
        }
        Ok(())
    }
}

pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// CRC-16/CCITT-FALSE: poly 0x1021, init 0xFFFF, no input/output reflection,
/// no final XOR.
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// CRC-8 as used by SMBus: poly 0x07, init 0x00, no reflection, no final XOR.
pub fn crc8_smbus(data: &[u8]) -> u8 {
    let mut crc: u8 = 0x00;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            if crc & 0x80 != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// CRC-32 (IEEE 802.3), the zlib/PNG polynomial: reflected in and out,
/// init 0xFFFFFFFF, final complement.
pub fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_modbus_check_value() {
        // Standard CRC catalogue check value for CRC-16/MODBUS.
        assert_eq!(crc16_modbus(b"123456789"), 0x4B37);
    }

    #[test]
    fn crc16_wire_order_is_little_endian() {
        assert_eq!(ChecksumKind::Crc16Modbus.compute(b"123456789"), vec![0x37, 0x4B]);
    }

    #[test]
    fn xor_and_sum() {
        assert_eq!(ChecksumKind::Xor8.compute(&[0x01, 0x02, 0x04]), vec![0x07]);
        assert_eq!(ChecksumKind::Sum8.compute(&[0xFF, 0x02]), vec![0x01]);
    }

    #[test]
    fn verify_frame_detects_corruption() {
        let mut frame = b"123456789".to_vec();
        frame.extend(ChecksumKind::Crc16Modbus.compute(b"123456789"));
        assert!(ChecksumKind::Crc16Modbus.verify_frame(&frame).is_ok());
        frame[0] ^= 0xFF;
        assert!(ChecksumKind::Crc16Modbus.verify_frame(&frame).is_err());
    }

    #[test]
    fn unknown_name_is_loud() {
        let err = ChecksumKind::from_name("crc64").unwrap_err();
        let msg = err.to_string();
        for name in ["crc16_modbus", "crc16_ccitt", "crc8", "crc32", "xor8", "sum8", "sum16be"] {
            assert!(msg.contains(name), "error must list {name}: {msg}");
        }
    }

    #[test]
    fn crc16_ccitt_known_answer() {
        // Standard CRC catalogue check value for CRC-16/CCITT-FALSE.
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
        // Big-endian wire order at the frame tail.
        assert_eq!(ChecksumKind::Crc16Ccitt.compute(b"123456789"), vec![0x29, 0xB1]);
        assert_eq!(ChecksumKind::from_name("crc16_ccitt").unwrap(), ChecksumKind::Crc16Ccitt);
        assert_eq!(ChecksumKind::Crc16Ccitt.name(), "crc16_ccitt");
        assert_eq!(ChecksumKind::Crc16Ccitt.len(), 2);
    }

    #[test]
    fn crc8_known_answer() {
        // Standard CRC catalogue check value for CRC-8 (SMBus).
        assert_eq!(ChecksumKind::Crc8.compute(b"123456789"), vec![0xF4]);
        assert_eq!(ChecksumKind::from_name("crc8").unwrap(), ChecksumKind::Crc8);
        assert_eq!(ChecksumKind::Crc8.name(), "crc8");
        assert_eq!(ChecksumKind::Crc8.len(), 1);
    }

    #[test]
    fn crc32_known_answer() {
        // Standard CRC catalogue check value for CRC-32 (IEEE, zlib).
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF43926);
        // Little-endian wire order at the frame tail.
        assert_eq!(
            ChecksumKind::Crc32.compute(b"123456789"),
            vec![0x26, 0x39, 0xF4, 0xCB]
        );
        assert_eq!(ChecksumKind::from_name("crc32").unwrap(), ChecksumKind::Crc32);
        assert_eq!(ChecksumKind::Crc32.name(), "crc32");
        assert_eq!(ChecksumKind::Crc32.len(), 4);
    }

    #[test]
    fn new_variants_verify_frames() {
        for kind in [ChecksumKind::Crc16Ccitt, ChecksumKind::Crc8, ChecksumKind::Crc32] {
            let mut frame = b"123456789".to_vec();
            frame.extend(kind.compute(b"123456789"));
            assert!(kind.verify_frame(&frame).is_ok(), "{}", kind.name());
            frame[0] ^= 0xFF;
            assert!(kind.verify_frame(&frame).is_err(), "{}", kind.name());
        }
    }

    #[test]
    fn sum16be_known_vector() {
        // PMS5003-style: 0x42 + 0x4D + 0x00 + 0x1C = 0x00AB, big-endian tail.
        assert_eq!(
            ChecksumKind::Sum16Be.compute(&[0x42, 0x4D, 0x00, 0x1C]),
            vec![0x00, 0xAB]
        );
        // Sum overflowing one byte: 0xFF * 3 = 0x02FD.
        assert_eq!(ChecksumKind::Sum16Be.compute(&[0xFF, 0xFF, 0xFF]), vec![0x02, 0xFD]);
        assert_eq!(ChecksumKind::from_name("sum16be").unwrap(), ChecksumKind::Sum16Be);
        assert_eq!(ChecksumKind::Sum16Be.len(), 2);

        let mut frame = vec![0x42, 0x4D, 0x00, 0x1C];
        frame.extend(ChecksumKind::Sum16Be.compute(&[0x42, 0x4D, 0x00, 0x1C]));
        assert!(ChecksumKind::Sum16Be.verify_frame(&frame).is_ok());
        frame[2] ^= 0x01;
        assert!(ChecksumKind::Sum16Be.verify_frame(&frame).is_err());
    }
}
