//! Checksum algorithms supported by the v0 command format. Each checksum is
//! appended to the frame tail; CRC16/MODBUS is emitted little-endian per the
//! Modbus RTU wire convention.

use crate::{hex, CoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumKind {
    Crc16Modbus,
    Xor8,
    Sum8,
    /// 16-bit sum of all bytes, emitted as two big-endian bytes (PMS5003 style).
    Sum16Be,
}

impl ChecksumKind {
    pub fn from_name(name: &str) -> crate::Result<Self> {
        match name {
            "crc16_modbus" => Ok(Self::Crc16Modbus),
            "xor8" => Ok(Self::Xor8),
            "sum8" => Ok(Self::Sum8),
            "sum16be" => Ok(Self::Sum16Be),
            other => Err(CoreError::UnknownChecksum(other.to_string())),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Crc16Modbus => "crc16_modbus",
            Self::Xor8 => "xor8",
            Self::Sum8 => "sum8",
            Self::Sum16Be => "sum16be",
        }
    }

    /// Number of bytes this checksum occupies at the frame tail.
    #[allow(clippy::len_without_is_empty)] // a checksum width is never zero
    pub fn len(&self) -> usize {
        match self {
            Self::Crc16Modbus | Self::Sum16Be => 2,
            Self::Xor8 | Self::Sum8 => 1,
        }
    }

    pub fn compute(&self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Crc16Modbus => crc16_modbus(data).to_le_bytes().to_vec(),
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
        assert!(ChecksumKind::from_name("crc32").is_err());
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
