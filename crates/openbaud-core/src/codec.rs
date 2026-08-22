//! Binary field types: encoding of typed parameters into frame bytes and
//! decoding of response fields at byte offsets.

use crate::CoreError;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    U8,
    I8,
    U16Be,
    U16Le,
    I16Be,
    I16Le,
    U32Be,
    U32Le,
    I32Be,
    I32Le,
    F32Be,
    F32Le,
    /// CDAB word-swapped ("mid-endian"): low 16-bit word first on the wire,
    /// big-endian within each word. Common in Modbus energy meters.
    U32Me,
    I32Me,
    F32Me,
    /// Text-frame only: interpolated as decimal integer, no binary size.
    Int,
    /// Text-frame only: interpolated as decimal float, no binary size.
    Float,
    /// Text-frame only: interpolated verbatim.
    Str,
}

impl FieldType {
    pub fn from_name(name: &str) -> crate::Result<Self> {
        Ok(match name {
            "u8" => Self::U8,
            "i8" => Self::I8,
            "u16be" => Self::U16Be,
            "u16le" => Self::U16Le,
            "i16be" => Self::I16Be,
            "i16le" => Self::I16Le,
            "u32be" => Self::U32Be,
            "u32le" => Self::U32Le,
            "i32be" => Self::I32Be,
            "i32le" => Self::I32Le,
            "f32be" => Self::F32Be,
            "f32le" => Self::F32Le,
            "u32me" => Self::U32Me,
            "i32me" => Self::I32Me,
            "f32me" => Self::F32Me,
            "int" => Self::Int,
            "float" => Self::Float,
            "string" => Self::Str,
            other => return Err(CoreError::UnknownFieldType(other.to_string())),
        })
    }

    /// Binary width in bytes; None for text-only types.
    pub fn size(&self) -> Option<usize> {
        match self {
            Self::U8 | Self::I8 => Some(1),
            Self::U16Be | Self::U16Le | Self::I16Be | Self::I16Le => Some(2),
            Self::U32Be
            | Self::U32Le
            | Self::I32Be
            | Self::I32Le
            | Self::F32Be
            | Self::F32Le
            | Self::U32Me
            | Self::I32Me
            | Self::F32Me => Some(4),
            Self::Int | Self::Float | Self::Str => None,
        }
    }

    pub fn encode(&self, name: &str, value: &Value) -> crate::Result<Vec<u8>> {
        let err = |reason: String| CoreError::Param { name: name.to_string(), reason };
        let as_i64 = |v: &Value| -> crate::Result<i64> {
            v.as_i64().ok_or_else(|| err(format!("expected an integer, got {v}")))
        };
        Ok(match self {
            Self::U8 => {
                let n = as_i64(value)?;
                u8::try_from(n).map_err(|_| err(format!("{n} out of range for u8")))?.to_be_bytes().to_vec()
            }
            Self::I8 => {
                let n = as_i64(value)?;
                i8::try_from(n).map_err(|_| err(format!("{n} out of range for i8")))?.to_be_bytes().to_vec()
            }
            Self::U16Be | Self::U16Le => {
                let n = as_i64(value)?;
                let v = u16::try_from(n).map_err(|_| err(format!("{n} out of range for u16")))?;
                if matches!(self, Self::U16Be) { v.to_be_bytes().to_vec() } else { v.to_le_bytes().to_vec() }
            }
            Self::I16Be | Self::I16Le => {
                let n = as_i64(value)?;
                let v = i16::try_from(n).map_err(|_| err(format!("{n} out of range for i16")))?;
                if matches!(self, Self::I16Be) { v.to_be_bytes().to_vec() } else { v.to_le_bytes().to_vec() }
            }
            Self::U32Be | Self::U32Le => {
                let n = as_i64(value)?;
                let v = u32::try_from(n).map_err(|_| err(format!("{n} out of range for u32")))?;
                if matches!(self, Self::U32Be) { v.to_be_bytes().to_vec() } else { v.to_le_bytes().to_vec() }
            }
            Self::I32Be | Self::I32Le => {
                let n = as_i64(value)?;
                let v = i32::try_from(n).map_err(|_| err(format!("{n} out of range for i32")))?;
                if matches!(self, Self::I32Be) { v.to_be_bytes().to_vec() } else { v.to_le_bytes().to_vec() }
            }
            Self::F32Be | Self::F32Le => {
                let f = value.as_f64().ok_or_else(|| err(format!("expected a number, got {value}")))? as f32;
                if matches!(self, Self::F32Be) { f.to_be_bytes().to_vec() } else { f.to_le_bytes().to_vec() }
            }
            Self::U32Me => {
                let n = as_i64(value)?;
                let v = u32::try_from(n).map_err(|_| err(format!("{n} out of range for u32")))?;
                cdab_swap(v.to_be_bytes()).to_vec()
            }
            Self::I32Me => {
                let n = as_i64(value)?;
                let v = i32::try_from(n).map_err(|_| err(format!("{n} out of range for i32")))?;
                cdab_swap(v.to_be_bytes()).to_vec()
            }
            Self::F32Me => {
                let f = value.as_f64().ok_or_else(|| err(format!("expected a number, got {value}")))? as f32;
                cdab_swap(f.to_be_bytes()).to_vec()
            }
            Self::Int | Self::Float | Self::Str => {
                return Err(err("text-only type cannot be encoded into a hex frame".to_string()))
            }
        })
    }

    /// Decode a binary field at `at` in `data`. Returns a JSON number.
    pub fn decode(&self, data: &[u8], at: usize) -> crate::Result<Value> {
        let size = self.size().ok_or_else(|| {
            CoreError::Parse("text-only type cannot be decoded from binary".to_string())
        })?;
        let slice = data.get(at..at + size).ok_or_else(|| {
            CoreError::Parse(format!(
                "field at byte {at} (width {size}) exceeds response length {}",
                data.len()
            ))
        })?;
        let val = match self {
            Self::U8 => Value::from(slice[0]),
            Self::I8 => Value::from(slice[0] as i8),
            Self::U16Be => Value::from(u16::from_be_bytes([slice[0], slice[1]])),
            Self::U16Le => Value::from(u16::from_le_bytes([slice[0], slice[1]])),
            Self::I16Be => Value::from(i16::from_be_bytes([slice[0], slice[1]])),
            Self::I16Le => Value::from(i16::from_le_bytes([slice[0], slice[1]])),
            Self::U32Be => Value::from(u32::from_be_bytes(slice.try_into().unwrap())),
            Self::U32Le => Value::from(u32::from_le_bytes(slice.try_into().unwrap())),
            Self::I32Be => Value::from(i32::from_be_bytes(slice.try_into().unwrap())),
            Self::I32Le => Value::from(i32::from_le_bytes(slice.try_into().unwrap())),
            Self::F32Be => Value::from(f32::from_be_bytes(slice.try_into().unwrap()) as f64),
            Self::F32Le => Value::from(f32::from_le_bytes(slice.try_into().unwrap()) as f64),
            Self::U32Me => Value::from(u32::from_be_bytes(cdab_swap(slice.try_into().unwrap()))),
            Self::I32Me => Value::from(i32::from_be_bytes(cdab_swap(slice.try_into().unwrap()))),
            Self::F32Me => {
                Value::from(f32::from_be_bytes(cdab_swap(slice.try_into().unwrap())) as f64)
            }
            Self::Int | Self::Float | Self::Str => unreachable!("guarded by size() above"),
        };
        Ok(val)
    }
}

/// CDAB word swap: logical big-endian bytes [A,B,C,D] <-> wire order [C,D,A,B].
/// The swap is its own inverse, so it serves both encode and decode.
fn cdab_swap(bytes: [u8; 4]) -> [u8; 4] {
    [bytes[2], bytes[3], bytes[0], bytes[1]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encode_endianness() {
        assert_eq!(FieldType::U16Be.encode("x", &json!(0x1234)).unwrap(), vec![0x12, 0x34]);
        assert_eq!(FieldType::U16Le.encode("x", &json!(0x1234)).unwrap(), vec![0x34, 0x12]);
    }

    #[test]
    fn encode_range_check_is_loud() {
        let err = FieldType::U8.encode("addr", &json!(300)).unwrap_err();
        assert!(err.to_string().contains("addr"));
    }

    #[test]
    fn decode_signed_and_offset() {
        let data = [0x00, 0xFF, 0xFE];
        assert_eq!(FieldType::I16Be.decode(&data, 1).unwrap(), json!(-2));
        assert!(FieldType::U32Be.decode(&data, 1).is_err());
    }

    #[test]
    fn u32me_word_swapped() {
        // Logical 2000 = 0x000007D0; CDAB wire order is 07 D0 00 00.
        let wire = [0x07, 0xD0, 0x00, 0x00];
        assert_eq!(FieldType::U32Me.decode(&wire, 0).unwrap(), json!(2000));
        assert_eq!(FieldType::U32Me.encode("x", &json!(2000)).unwrap(), wire.to_vec());
    }

    #[test]
    fn i32me_negative() {
        // -2 = 0xFFFFFFFE, big-endian [FF FF FF FE] -> CDAB [FF FE FF FF].
        let bytes = FieldType::I32Me.encode("x", &json!(-2)).unwrap();
        assert_eq!(bytes, vec![0xFF, 0xFE, 0xFF, 0xFF]);
        assert_eq!(FieldType::I32Me.decode(&bytes, 0).unwrap(), json!(-2));
    }

    #[test]
    fn f32me_round_trip() {
        let bytes = FieldType::F32Me.encode("x", &json!(220.5)).unwrap();
        assert_eq!(bytes.len(), 4);
        // Word-swapped relative to plain big-endian.
        assert_ne!(bytes, FieldType::F32Be.encode("x", &json!(220.5)).unwrap());
        assert_eq!(FieldType::F32Me.decode(&bytes, 0).unwrap(), json!(220.5));
    }

    #[test]
    fn text_types_reject_binary_use() {
        assert!(FieldType::Str.encode("s", &json!("hi")).is_err());
        assert!(FieldType::Int.decode(&[1, 2], 0).is_err());
    }
}
