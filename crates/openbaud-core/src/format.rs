//! The v0 knowledge formats: device profiles and named commands.
//!
//! Parsing is strict: unknown fields are rejected, semantic rules (exactly-one
//! framing mode, valid types, template placeholders all declared) are checked
//! at load time and reported with the file path.

use crate::checksum::ChecksumKind;
use crate::codec::FieldType;
use crate::framing::{Framing, MatchRule};
use crate::template::HexTemplate;
use crate::CoreError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

pub const PROFILE_SCHEMA: &str = "openbaud/profile@v0";
pub const COMMAND_SCHEMA: &str = "openbaud/command@v0";
pub const WORKFLOW_SCHEMA: &str = "openbaud/workflow@v0";

fn ferr(path: &str, reason: impl Into<String>) -> CoreError {
    CoreError::Format { path: path.to_string(), reason: reason.into() }
}

// ---------------------------------------------------------------------------
// Profile
// ---------------------------------------------------------------------------

/// A device profile: serial transport settings, response framing and the
/// stable identity used to resolve the physical port automatically.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Format identifier; must be "openbaud/profile@v0".
    pub schema: String,
    /// Device name; used as the directory name in the workspace.
    pub name: String,
    /// Free-form description of the device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Serial port parameters (baud, data bits, parity, stop bits).
    #[serde(default)]
    pub transport: Transport,
    /// Default framing for unsolicited/streamed device output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<FramingSpec>,
    /// USB identity for automatic port resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<SelectorSpec>,
}

/// Stable device identity for automatic port resolution: all present fields
/// must match (AND). vid/pid are hex, case-insensitive; product is a
/// substring match; serial_number is exact.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectorSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
}

impl SelectorSpec {
    fn is_empty(&self) -> bool {
        self.vid.is_none()
            && self.pid.is_none()
            && self.serial_number.is_none()
            && self.product.is_none()
    }
}

/// Serial line parameters. Defaults: 115200 8N1.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transport {
    /// Baud rate (default 115200).
    #[serde(default = "default_baud")]
    pub baud: u32,
    /// Data bits, 5..=8 (default 8).
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    /// Parity bit (default none).
    #[serde(default)]
    pub parity: Parity,
    /// Stop bits, 1 or 2 (default 1).
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
}

fn default_baud() -> u32 {
    115_200
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            baud: default_baud(),
            data_bits: default_data_bits(),
            parity: Parity::default(),
            stop_bits: default_stop_bits(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Parity {
    #[default]
    None,
    Even,
    Odd,
}

/// How to cut the byte stream into frames. Exactly one of the three modes
/// must be set.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FramingSpec {
    /// Frame ends with this byte sequence (given as text, e.g. "\r\n").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    /// Frame ends after this many milliseconds of line silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ms: Option<u64>,
    /// Frame length is read from a fixed-size header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_prefix: Option<LengthPrefixSpec>,
}

/// Length-prefixed framing: a `header_len`-byte header carries the payload
/// length at `len_at`; total frame = header + payload + `extra` trailer bytes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LengthPrefixSpec {
    /// Header size in bytes.
    pub header_len: usize,
    /// Byte offset of the length field within the header.
    pub len_at: usize,
    /// Width of the length field in bytes (1..=4).
    pub len_size: usize,
    /// Byte order of the length field (default big-endian).
    #[serde(default)]
    pub endian: Endian,
    /// Trailer bytes after the payload not counted by the length field.
    #[serde(default)]
    pub extra: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Endian {
    #[default]
    Be,
    Le,
}

impl FramingSpec {
    pub fn to_framing(&self, path: &str) -> crate::Result<Framing> {
        let modes = [
            self.delimiter.is_some(),
            self.idle_ms.is_some(),
            self.length_prefix.is_some(),
        ];
        if modes.iter().filter(|m| **m).count() != 1 {
            return Err(ferr(
                path,
                "framing must set exactly one of: delimiter, idle_ms, length_prefix",
            ));
        }
        if let Some(d) = &self.delimiter {
            if d.is_empty() {
                return Err(ferr(path, "framing.delimiter must not be empty"));
            }
            return Ok(Framing::Delimiter { delimiter: d.as_bytes().to_vec() });
        }
        if let Some(ms) = self.idle_ms {
            if ms == 0 {
                return Err(ferr(path, "framing.idle_ms must be > 0"));
            }
            return Ok(Framing::Idle { idle_ms: ms });
        }
        let lp = self.length_prefix.as_ref().expect("checked above");
        if lp.len_size == 0 || lp.len_size > 4 || lp.len_at + lp.len_size > lp.header_len {
            return Err(ferr(
                path,
                "length_prefix: require 1 <= len_size <= 4 and len_at + len_size <= header_len",
            ));
        }
        Ok(Framing::LengthPrefix {
            header_len: lp.header_len,
            len_at: lp.len_at,
            len_size: lp.len_size,
            big_endian: lp.endian == Endian::Be,
            extra: lp.extra,
        })
    }
}

pub fn parse_profile(yaml: &str, path: &str) -> crate::Result<Profile> {
    let profile: Profile =
        serde_yaml::from_str(yaml).map_err(|e| ferr(path, e.to_string()))?;
    if profile.schema != PROFILE_SCHEMA {
        return Err(ferr(
            path,
            format!("schema is {:?}, expected {PROFILE_SCHEMA:?}", profile.schema),
        ));
    }
    if profile.name.is_empty() {
        return Err(ferr(path, "name must not be empty"));
    }
    if !(5..=8).contains(&profile.transport.data_bits) {
        return Err(ferr(path, "transport.data_bits must be 5..=8"));
    }
    if !(1..=2).contains(&profile.transport.stop_bits) {
        return Err(ferr(path, "transport.stop_bits must be 1 or 2"));
    }
    if let Some(framing) = &profile.framing {
        framing.to_framing(path)?;
    }
    if let Some(selector) = &profile.selector {
        if selector.is_empty() {
            return Err(ferr(path, "selector must set at least one of: vid, pid, serial_number, product"));
        }
        for (key, value) in [("vid", &selector.vid), ("pid", &selector.pid)] {
            if let Some(v) = value {
                if v.is_empty() || v.len() > 4 || !v.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(ferr(path, format!("selector.{key} must be 1-4 hex digits, got {v:?}")));
                }
            }
        }
    }
    Ok(profile)
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

/// A named, parameterized device command: the TX frame template plus how to
/// match, validate and parse the response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// Format identifier; must be "openbaud/command@v0".
    pub schema: String,
    /// Command name; used as the file name in the device directory.
    pub name: String,
    /// Free-form description of what the command does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Risk class gating execution (default read).
    #[serde(default)]
    pub risk: Risk,
    /// Declared parameters usable in the frame template.
    #[serde(default)]
    pub params: Vec<ParamSpec>,
    /// The bytes to transmit.
    pub frame: FrameSpec,
    /// How to collect and interpret the reply; omit for fire-and-forget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseSpec>,
    /// Optional delays around the exchange.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<TimingSpec>,
    /// Where this knowledge came from and how it was verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

/// Optional command-level timing: delay before sending and quiet period after
/// the command completes. Absent field == 0.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimingSpec {
    /// Milliseconds to wait before transmitting.
    #[serde(default)]
    pub pre_delay_ms: u64,
    /// Quiet period in milliseconds after the command completes.
    #[serde(default)]
    pub post_delay_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    #[default]
    Read,
    Write,
    Danger,
}

/// One declared command parameter, referenced from the frame template as
/// `{name}`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamSpec {
    /// Parameter name, matching a `{name}` placeholder in the frame.
    pub name: String,
    /// Encoding type: binary (u8, u16be, ...) for hex frames; int, float or
    /// string for text frames.
    #[serde(rename = "type")]
    pub type_name: String,
    /// Value used when the caller does not supply one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Minimum accepted numeric value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum accepted numeric value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// What the parameter means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The TX frame template. Exactly one of `hex` / `text` must be set.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameSpec {
    /// Whitespace-separated hex bytes with `{param}` and checksum
    /// placeholders ({crc16_modbus}, {crc16_ccitt}, {crc8}, {crc32}, {xor8},
    /// {sum8}, {sum16be}); checksum placeholders are computed over every byte
    /// built before them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hex: Option<String>,
    /// Literal text with `{param}` interpolation; `{{`/`}}` escape braces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// How to collect the reply and what to do with it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResponseSpec {
    /// Required unless `expect: silence` (checked in `parse_command`).
    #[serde(rename = "match", default, skip_serializing_if = "Option::is_none")]
    pub match_spec: Option<MatchSpec>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// Which outcome the command declares as success. Defaults to `normal`.
    #[serde(default)]
    pub expect: Expect,
    /// Optional first-byte window: if no byte arrives within it, the engine
    /// may classify `silence` before `timeout_ms` elapses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_byte_ms: Option<u64>,
    /// Checksum verification of the collected frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validate: Option<ValidateSpec>,
    /// How to decode the frame into named values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse: Option<ParseSpec>,
    /// Optional rendering encoding: which parsed fields feed which visual
    /// channel. Absent means "no chart" — never a guess from field names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<ViewSpec>,
    /// Recognition and decoding of protocol-level exception frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exception: Option<ExceptionSpec>,
}

fn default_timeout_ms() -> u64 {
    3000
}

/// Declared success outcome for a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Expect {
    #[default]
    Normal,
    Exception,
    Silence,
}

/// How to recognize and decode a protocol exception frame (e.g. Modbus
/// FC|0x80 replies). `when` is a prefix test on the incoming bytes; once it
/// hits, the frame is collected with `match` and processed with
/// `validate`/`parse` — same semantics as the main response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExceptionSpec {
    pub when: WhenSpec,
    #[serde(rename = "match")]
    pub match_spec: MatchSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validate: Option<ValidateSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse: Option<ParseSpec>,
}

/// Byte-position predicate: `(byte[at] & mask) == equals`. `mask` and
/// `equals` are single hex bytes; `mask` defaults to "FF".
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WhenSpec {
    pub at: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    pub equals: String,
}

impl WhenSpec {
    /// The mask byte (defaults to 0xFF when unset).
    pub fn mask_byte(&self) -> crate::Result<u8> {
        match &self.mask {
            Some(m) => hex_byte(m),
            None => Ok(0xFF),
        }
    }

    pub fn equals_byte(&self) -> crate::Result<u8> {
        hex_byte(&self.equals)
    }
}

fn hex_byte(s: &str) -> crate::Result<u8> {
    let bytes = crate::hex::parse_hex(s)?;
    match bytes.as_slice() {
        [b] => Ok(*b),
        _ => Err(CoreError::InvalidHex {
            input: s.to_string(),
            reason: format!("expected exactly one byte, got {}", bytes.len()),
        }),
    }
}

/// When the collected reply counts as one complete frame. Exactly one of the
/// three modes must be set.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchSpec {
    /// Frame is complete after exactly this many bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<usize>,
    /// Frame ends with this byte sequence (given as text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    /// Frame ends after this many milliseconds of line silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ms: Option<u64>,
}

impl MatchSpec {
    pub fn to_rule(&self, path: &str) -> crate::Result<MatchRule> {
        let modes = [self.length.is_some(), self.delimiter.is_some(), self.idle_ms.is_some()];
        if modes.iter().filter(|m| **m).count() != 1 {
            return Err(ferr(path, "response.match must set exactly one of: length, delimiter, idle_ms"));
        }
        if let Some(n) = self.length {
            if n == 0 {
                return Err(ferr(path, "response.match.length must be > 0"));
            }
            return Ok(MatchRule::Length(n));
        }
        if let Some(d) = &self.delimiter {
            if d.is_empty() {
                return Err(ferr(path, "response.match.delimiter must not be empty"));
            }
            return Ok(MatchRule::Delimiter(d.as_bytes().to_vec()));
        }
        let ms = self.idle_ms.expect("checked above");
        if ms == 0 {
            return Err(ferr(path, "response.match.idle_ms must be > 0"));
        }
        Ok(MatchRule::Idle { idle_ms: ms })
    }
}

/// Checksum verification of a received frame. Defaults reproduce the classic
/// tail checksum: computed over every byte before the checksum value, which
/// sits at the frame tail as raw bytes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateSpec {
    /// Checksum algorithm: crc16_modbus, crc16_ccitt, crc8, crc32, xor8,
    /// sum8 or sum16be.
    pub checksum: String,
    /// Byte range the checksum is computed over, both ends inclusive.
    /// Negative indices count from the frame end (-1 = last byte).
    /// Default: byte 0 through the byte before the checksum value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<RangeSpec>,
    /// Position of the stored checksum value in the frame; negative counts
    /// from the frame end. Default: the frame tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<i64>,
    /// How the checksum value appears in the frame: raw bytes (default) or
    /// ASCII hex characters, two per checksum byte, compared
    /// case-insensitively (NMEA-style `*4A`).
    #[serde(default)]
    pub encoding: Encoding,
}

/// Inclusive byte range; negative indices count from the frame end
/// (-1 = last byte).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RangeSpec {
    /// First byte of the range (inclusive).
    pub from: i64,
    /// Last byte of the range (inclusive).
    pub to: i64,
}

/// On-wire representation of a checksum value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    /// The checksum's bytes appear verbatim.
    #[default]
    Raw,
    /// The checksum appears as ASCII hex characters (two per byte),
    /// compared case-insensitively.
    AsciiHex,
}

/// How a parsed result should be drawn. Declares which parsed field feeds
/// which visual channel, so every device keeps its own vocabulary — openbaud
/// never infers a chart from field names. Channel requirements are per kind
/// and enforced in `parse_command` (see COMMAND_RULES).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewSpec {
    pub kind: ViewKind,
    /// Parse field holding the record array the channels index into.
    pub data: String,
    /// polar: element field carrying the angle. Required for polar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// polar: element field carrying the distance from origin. Required for polar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<String>,
    /// polar: element field shading each point. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    /// Angular scan drawn on polar axes. The only kind a command YAML may
    /// declare.
    Polar,
    /// Session timeline produced by the `session_timeline` tool.
    Timeline,
    /// Frame diagnosis produced by the `diagnose_frame` tool.
    Diagnostics,
    /// Captured traffic produced by the `capture_frames` tool.
    Capture,
}

impl ViewKind {
    /// The snake_case name used on the wire and in error messages.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Polar => "polar",
            Self::Timeline => "timeline",
            Self::Diagnostics => "diagnostics",
            Self::Capture => "capture",
        }
    }
}

/// How to decode a frame into named values. Exactly one of `fields` (binary)
/// / `regex` (text) must be set.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParseSpec {
    /// Binary decode: field name -> byte layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, FieldSpec>>,
    /// Text decode: regex with named captures applied to the frame text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    /// Type coercion per named capture: a bare type name (int, float, string
    /// or hex_int; default string), or an object
    /// `{type, bits?, scale?, offset?}` sharing the binary decode pipeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<BTreeMap<String, TextType>>,
    /// Split a named capture into an array: capture name -> separator and
    /// element type. A capture cannot appear in both `types` and `split`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<BTreeMap<String, SplitSpec>>,
}

/// How to split one named regex capture into an array of converted values.
/// Empty segments (e.g. from trailing separators) are skipped.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitSpec {
    /// Separator string the capture is split on.
    pub sep: String,
    /// Element type: a bare type name (int, float, string or hex_int), or an
    /// object `{type, bits?, scale?, offset?}` applied per element.
    #[serde(rename = "type")]
    pub type_spec: TextType,
}

/// A text-side coercion type: either a bare type name, or an object adding
/// bit extraction and the linear transform to the conversion.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TextType {
    /// Bare type name: int, float, string or hex_int.
    Name(String),
    /// Type name plus optional bits / scale / offset, evaluated as
    /// `value = ((converted >> lsb) & mask(width)) x scale + offset`.
    Spec(TextTypeSpec),
}

/// Object form of a text-side coercion type. `bits` requires an integer type
/// (int or hex_int) and errors at runtime on negative converted values;
/// `scale`/`offset` require a numeric type; none of the three apply to
/// `string`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextTypeSpec {
    /// Base type: int, float, string or hex_int.
    #[serde(rename = "type")]
    pub type_name: String,
    /// Extract a bit sub-field of the converted integer before scaling.
    /// Only valid for int and hex_int; negative values are a runtime error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits: Option<BitsSpec>,
    /// Multiply the converted number by this factor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Added after scaling: `value = converted x scale + offset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
}

/// A bit sub-field of an unsigned integer value, evaluated as
/// `((raw >> lsb) & mask(width))` before scale/offset. `lsb`/`width` map
/// directly onto register-table `[msb:lsb]` notation:
/// `lsb` is the table's lsb and `width = msb - lsb + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BitsSpec {
    /// Lowest bit of the sub-field (0 = least significant bit of the value).
    pub lsb: u32,
    /// Number of bits (>= 1); `lsb + width` must fit the field's bit width.
    pub width: u32,
}

/// One decoded binary field — a scalar at a byte offset, or (with `count`) an
/// array of scalars or records.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    /// Byte offset in the frame; inside `elements`, offset within the record.
    pub at: usize,
    /// Scalar type: u8, i8, u16be/le, i16be/le, u32be/le/me, i32be/le/me,
    /// f32be/le/me, or ascii_int (requires `len`). Mutually exclusive with
    /// `elements`.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    /// Byte length of an ascii_int field; required by and only valid for
    /// type ascii_int.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len: Option<usize>,
    /// Multiply the decoded number by this factor (applied per element for
    /// arrays). Setting scale or offset makes the value a float.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Added after scaling — the canonical linear calibration is
    /// `value = raw x scale + offset` (scale defaults to 1, offset to 0).
    /// Setting scale or offset makes the value a float.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    /// Extract a bit sub-field of the unsigned decoded value before scaling:
    /// `value = ((raw >> lsb) & mask(width)) x scale + offset`. Only valid on
    /// unsigned integer types (u8, u16be, u16le, u32be, u32le, u32me);
    /// several fields may read different bits of the same byte offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits: Option<BitsSpec>,
    /// Unit label reported alongside the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Makes this field an array: a fixed element count, or `{field: name}`
    /// referencing a scalar field of the same parse block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<CountSpec>,
    /// Bytes from one element start to the next. Defaults to the scalar
    /// type's width; required with `elements`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stride: Option<usize>,
    /// Record-array layout: element field name -> scalar spec whose `at` is
    /// the offset within one record. Arrays nest only one level. Mutually
    /// exclusive with `type`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elements: Option<BTreeMap<String, FieldSpec>>,
}

/// Element count of an array field.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum CountSpec {
    /// Fixed number of elements (must be >= 1).
    Fixed(i64),
    /// Count read from a scalar field decoded from the same frame.
    Field(CountFieldRef),
}

/// Reference to the scalar field that carries an array's element count.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CountFieldRef {
    /// Name of the scalar field in the same parse block.
    pub field: String,
}

/// Where this command's knowledge came from.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// Datasheet reference, e.g. "datasheet.pdf#page=7".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datasheet: Option<String>,
    /// Evidence that the command was verified against a real device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<Verified>,
}

/// Real-device verification evidence.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Verified {
    /// Path of the capture file that proves the exchange.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<String>,
    /// Free-form verification note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// When the verification happened (ISO date).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

impl Command {
    /// Declared parameter types, resolved. Only valid after `parse_command`.
    pub fn param_types(&self) -> HashMap<String, FieldType> {
        self.params
            .iter()
            .filter_map(|p| FieldType::from_name(&p.type_name).ok().map(|t| (p.name.clone(), t)))
            .collect()
    }
}

pub fn parse_command(yaml: &str, path: &str) -> crate::Result<Command> {
    let cmd: Command = serde_yaml::from_str(yaml).map_err(|e| ferr(path, e.to_string()))?;
    if cmd.schema != COMMAND_SCHEMA {
        return Err(ferr(path, format!("schema is {:?}, expected {COMMAND_SCHEMA:?}", cmd.schema)));
    }
    if cmd.name.is_empty() {
        return Err(ferr(path, "name must not be empty"));
    }

    // Params: valid types, no duplicates.
    let mut declared: HashMap<&str, FieldType> = HashMap::new();
    for p in &cmd.params {
        let ty = FieldType::from_name(&p.type_name)
            .map_err(|e| ferr(path, format!("param {:?}: {e}", p.name)))?;
        if ty == FieldType::AsciiInt {
            return Err(ferr(
                path,
                format!("param {:?}: ascii_int is a response-field type, not a param type", p.name),
            ));
        }
        if declared.insert(p.name.as_str(), ty).is_some() {
            return Err(ferr(path, format!("duplicate param {:?}", p.name)));
        }
    }

    // Frame: exactly one of hex/text; hex templates must parse and only use
    // declared, binary-encodable params.
    match (&cmd.frame.hex, &cmd.frame.text) {
        (Some(hex), None) => {
            let template = HexTemplate::parse(hex).map_err(|e| ferr(path, format!("frame.hex: {e}")))?;
            for name in template.param_names() {
                match declared.get(name) {
                    None => {
                        return Err(ferr(path, format!("frame.hex references undeclared param {name:?}")))
                    }
                    Some(ty) if ty.size().is_none() => {
                        return Err(ferr(
                            path,
                            format!("param {name:?} has text-only type {:?}, not usable in a hex frame",
                                ty),
                        ))
                    }
                    Some(_) => {}
                }
            }
        }
        (None, Some(_)) => {}
        _ => return Err(ferr(path, "frame must set exactly one of: hex, text")),
    }

    // Response rules.
    if let Some(resp) = &cmd.response {
        match (&resp.match_spec, resp.expect) {
            (Some(m), _) => {
                m.to_rule(path)?;
            }
            (None, Expect::Silence) => {}
            (None, _) => {
                return Err(ferr(path, "response.match is required unless expect: silence"))
            }
        }
        if resp.first_byte_ms == Some(0) {
            return Err(ferr(path, "response.first_byte_ms must be > 0"));
        }
        if resp.expect == Expect::Exception && resp.exception.is_none() {
            return Err(ferr(
                path,
                "expect: exception requires a response.exception block to recognize the frame",
            ));
        }
        if let Some(v) = &resp.validate {
            check_validate_spec(v, path, "validate")?;
        }
        if let Some(parse) = &resp.parse {
            check_parse_spec(parse, path, "parse")?;
        }
        if let Some(view) = &resp.view {
            check_view_spec(view, resp.parse.as_ref(), path)?;
        }
        if let Some(exc) = &resp.exception {
            exc.when
                .mask_byte()
                .map_err(|e| ferr(path, format!("exception.when.mask: {e}")))?;
            exc.when
                .equals_byte()
                .map_err(|e| ferr(path, format!("exception.when.equals: {e}")))?;
            exc.match_spec.to_rule(path)?;
            if let Some(v) = &exc.validate {
                check_validate_spec(v, path, "exception.validate")?;
            }
            if let Some(parse) = &exc.parse {
                check_parse_spec(parse, path, "exception.parse")?;
            }
        }
    }
    Ok(cmd)
}

/// Static checks for a `validate` block: known checksum name, and range
/// inversion when both ends are non-negative (mixed signs are resolved at
/// runtime against the actual frame length).
fn check_validate_spec(v: &ValidateSpec, path: &str, ctx: &str) -> crate::Result<()> {
    ChecksumKind::from_name(&v.checksum).map_err(|e| ferr(path, format!("{ctx}: {e}")))?;
    if let Some(r) = &v.range {
        if r.from >= 0 && r.to >= 0 && r.from > r.to {
            return Err(ferr(
                path,
                format!("{ctx}.range: from {} is after to {}", r.from, r.to),
            ));
        }
    }
    Ok(())
}

/// Shared semantic checks for a `parse` block (main response and exception).
/// `ctx` names the block in error messages ("parse" or "exception.parse").
/// Units that clearly mark a field as an angle or as a length. Used only to
/// catch the mistake that actually happens — angle and radius declared the
/// wrong way round. An undeclared unit is never an error.
fn unit_class(unit: &str) -> Option<&'static str> {
    match unit.trim().to_ascii_lowercase().as_str() {
        "deg" | "degree" | "degrees" | "rad" | "radian" | "radians" => Some("angle"),
        "mm" | "cm" | "m" | "metre" | "meter" | "metres" | "meters" | "in" | "inch" => {
            Some("length")
        }
        _ => None,
    }
}

/// A view declares which parsed field feeds which visual channel. Every channel
/// must name a real field, so a renaming breaks the build instead of silently
/// dropping the chart.
fn check_view_spec(view: &ViewSpec, parse: Option<&ParseSpec>, path: &str) -> crate::Result<()> {
    // Kind gate first, so a tool-produced kind gets this message and not a
    // complaint about its channels.
    match view.kind {
        ViewKind::Polar => {}
        ViewKind::Timeline | ViewKind::Diagnostics | ViewKind::Capture => {
            return Err(ferr(
                path,
                format!(
                    "response.view.kind {}: this view kind is produced by tools \
                     (session_timeline / diagnose_frame / capture_frames), not declared in \
                     command YAML — polar is the only declarable kind",
                    view.kind.name()
                ),
            ));
        }
    }
    let fields = parse.and_then(|p| p.fields.as_ref()).ok_or_else(|| {
        ferr(path, "response.view needs response.parse.fields to name channels in")
    })?;
    let data = fields.get(&view.data).ok_or_else(|| {
        ferr(path, format!("response.view.data {:?} is not a parse field", view.data))
    })?;
    let elements = data.elements.as_ref().ok_or_else(|| {
        ferr(
            path,
            format!(
                "response.view.data {:?} is not a record array — a view maps channels onto the \
                 elements of an array field (one with `count` and `elements`)",
                view.data
            ),
        )
    })?;

    let channel = |name: &str, field: &Option<String>, want: &str| -> crate::Result<()> {
        let Some(field_name) = field else { return Ok(()) };
        let spec = elements.get(field_name).ok_or_else(|| {
            ferr(
                path,
                format!(
                    "response.view.{name} {field_name:?} is not an element of {:?}",
                    view.data
                ),
            )
        })?;
        if let Some(class) = spec.unit.as_deref().and_then(unit_class) {
            if class != want {
                return Err(ferr(
                    path,
                    format!(
                        "response.view.{name} {field_name:?} has unit {:?} ({class}) but the \
                         {name} channel takes a {want} — are angle and radius swapped?",
                        spec.unit.as_deref().unwrap_or_default()
                    ),
                ));
            }
        }
        Ok(())
    };

    // Only polar reaches this point (the kind gate above returned for the
    // tool-produced kinds).
    if view.angle.is_none() || view.radius.is_none() {
        return Err(ferr(
            path,
            "response.view kind polar requires both an angle and a radius channel",
        ));
    }
    channel("angle", &view.angle, "angle")?;
    channel("radius", &view.radius, "length")?;
    // intensity is a bare magnitude: any unit (or none) is fine.
    if let Some(name) = &view.intensity {
        if !elements.contains_key(name) {
            return Err(ferr(
                path,
                format!(
                    "response.view.intensity {name:?} is not an element of {:?}",
                    view.data
                ),
            ));
        }
    }
    Ok(())
}

fn check_parse_spec(parse: &ParseSpec, path: &str, ctx: &str) -> crate::Result<()> {
    match (&parse.fields, &parse.regex) {
        (Some(_), Some(_)) => {
            return Err(ferr(path, format!("{ctx} must set fields or regex, not both")))
        }
        (None, None) => return Err(ferr(path, format!("{ctx} must set one of: fields, regex"))),
        (Some(fields), None) => {
            if parse.split.is_some() {
                return Err(ferr(path, format!("{ctx}.split only applies to regex parsing")));
            }
            for (fname, f) in fields {
                check_field_spec(f, fields, path, &format!("{ctx}.fields.{fname}"))?;
            }
        }
        (None, Some(re)) => {
            let regex = regex::Regex::new(re)
                .map_err(|e| ferr(path, format!("{ctx}.regex does not compile: {e}")))?;
            if let Some(split) = &parse.split {
                let captures: Vec<&str> = regex.capture_names().flatten().collect();
                for (name, s) in split {
                    if !captures.contains(&name.as_str()) {
                        return Err(ferr(
                            path,
                            format!("{ctx}.split.{name}: regex has no named capture {name:?}"),
                        ));
                    }
                    if s.sep.is_empty() {
                        return Err(ferr(path, format!("{ctx}.split.{name}.sep must not be empty")));
                    }
                    check_text_type(&s.type_spec, path, &format!("{ctx}.split.{name}.type"))?;
                    if parse.types.as_ref().is_some_and(|t| t.contains_key(name)) {
                        return Err(ferr(
                            path,
                            format!("{ctx}: capture {name:?} is declared in both types and split"),
                        ));
                    }
                }
            }
        }
    }
    if let Some(types) = &parse.types {
        for (name, t) in types {
            check_text_type(t, path, &format!("{ctx}.types.{name}"))?;
        }
    }
    Ok(())
}

/// Allowed text-side coercion types (regex `types` and `split.type`): a bare
/// type name, or an object whose bits/scale/offset must fit the base type.
fn check_text_type(t: &TextType, path: &str, ctx: &str) -> crate::Result<()> {
    match t {
        TextType::Name(name) => check_text_type_name(name, path, ctx),
        TextType::Spec(spec) => {
            check_text_type_name(&spec.type_name, path, ctx)?;
            match spec.type_name.as_str() {
                "string" => {
                    if spec.bits.is_some() || spec.scale.is_some() || spec.offset.is_some() {
                        return Err(ferr(
                            path,
                            format!("{ctx}: bits/scale/offset do not apply to type string"),
                        ));
                    }
                }
                "float" if spec.bits.is_some() => {
                    return Err(ferr(
                        path,
                        format!("{ctx}: bits requires an integer type (int, hex_int), not float"),
                    ));
                }
                _ => {} // int / hex_int: bits, scale and offset all apply
            }
            if let Some(bits) = &spec.bits {
                // Text integers are non-negative i64 at extraction time.
                check_bits(bits, 63, "text integer", path, ctx)?;
            }
            Ok(())
        }
    }
}

/// The four allowed text-side base type names.
fn check_text_type_name(t: &str, path: &str, ctx: &str) -> crate::Result<()> {
    if !matches!(t, "int" | "float" | "string" | "hex_int") {
        return Err(ferr(
            path,
            format!("{ctx}: {t:?} is not one of int, float, string, hex_int"),
        ));
    }
    Ok(())
}

/// Shared bits sanity checks: non-zero width, and the sub-field must fit the
/// carrier's bit width. `what` names the carrier in the error message.
fn check_bits(bits: &BitsSpec, carrier_bits: u32, what: &str, path: &str, ctx: &str) -> crate::Result<()> {
    if bits.width == 0 {
        return Err(ferr(path, format!("{ctx}: bits.width must be >= 1")));
    }
    if u64::from(bits.lsb) + u64::from(bits.width) > u64::from(carrier_bits) {
        return Err(ferr(
            path,
            format!(
                "{ctx}: bits [msb:lsb] = [{}:{}] exceeds the {carrier_bits}-bit {what}",
                u64::from(bits.lsb) + u64::from(bits.width) - 1,
                bits.lsb
            ),
        ));
    }
    Ok(())
}

/// Static checks for one binary field: scalar type rules, and — when `count`
/// is set — the array rules (count source, stride, one-level `elements`).
fn check_field_spec(
    f: &FieldSpec,
    all: &BTreeMap<String, FieldSpec>,
    path: &str,
    ctx: &str,
) -> crate::Result<()> {
    match (&f.type_name, &f.elements) {
        (Some(_), Some(_)) => {
            return Err(ferr(path, format!("{ctx}: type and elements are mutually exclusive")))
        }
        (None, None) => {
            return Err(ferr(path, format!("{ctx}: must set one of: type, elements")))
        }
        (Some(_), None) => {
            let width = check_scalar_type(f, path, ctx)?;
            if let (Some(stride), Some(_)) = (f.stride, &f.count) {
                if stride < width {
                    return Err(ferr(
                        path,
                        format!("{ctx}: stride {stride} is smaller than the element width {width}"),
                    ));
                }
            }
        }
        (None, Some(elements)) => {
            if f.count.is_none() {
                return Err(ferr(path, format!("{ctx}: elements requires count")));
            }
            if f.len.is_some() {
                return Err(ferr(path, format!("{ctx}: len is only valid for type ascii_int")));
            }
            if f.scale.is_some() || f.offset.is_some() || f.bits.is_some() || f.unit.is_some() {
                return Err(ferr(
                    path,
                    format!(
                        "{ctx}: scale/offset/bits/unit belong on the element fields of a record array"
                    ),
                ));
            }
            let stride = f
                .stride
                .ok_or_else(|| ferr(path, format!("{ctx}: stride is required with elements")))?;
            if stride == 0 {
                return Err(ferr(path, format!("{ctx}: stride must be >= 1")));
            }
            if elements.is_empty() {
                return Err(ferr(path, format!("{ctx}: elements must not be empty")));
            }
            for (ename, e) in elements {
                let ectx = format!("{ctx}.elements.{ename}");
                if e.count.is_some() || e.elements.is_some() || e.stride.is_some() {
                    return Err(ferr(
                        path,
                        format!("{ectx}: arrays nest only one level; element fields must be scalars"),
                    ));
                }
                if e.type_name.is_none() {
                    return Err(ferr(path, format!("{ectx}: must set type")));
                }
                let width = check_scalar_type(e, path, &ectx)?;
                if e.at + width > stride {
                    return Err(ferr(
                        path,
                        format!(
                            "{ectx}: at {} + width {width} exceeds the record stride {stride}",
                            e.at
                        ),
                    ));
                }
            }
        }
    }
    match &f.count {
        None => {
            if f.stride.is_some() {
                return Err(ferr(path, format!("{ctx}: stride requires count")));
            }
        }
        Some(CountSpec::Fixed(n)) => {
            if *n < 1 {
                return Err(ferr(path, format!("{ctx}: count must be >= 1, got {n}")));
            }
        }
        Some(CountSpec::Field(r)) => match all.get(&r.field) {
            None => {
                return Err(ferr(
                    path,
                    format!("{ctx}: count field {:?} is not declared in this parse", r.field),
                ))
            }
            Some(target) if target.count.is_some() || target.elements.is_some() => {
                return Err(ferr(
                    path,
                    format!("{ctx}: count field {:?} must be a scalar field, not an array", r.field),
                ))
            }
            Some(_) => {}
        },
    }
    Ok(())
}

/// Check a scalar field's type/len/bits combination and return its byte width.
fn check_scalar_type(f: &FieldSpec, path: &str, ctx: &str) -> crate::Result<usize> {
    let name = f.type_name.as_ref().expect("caller ensures type is present");
    let ty = FieldType::from_name(name).map_err(|e| ferr(path, format!("{ctx}: {e}")))?;
    if let Some(bits) = &f.bits {
        match ty.unsigned_bits() {
            None => {
                return Err(ferr(
                    path,
                    format!(
                        "{ctx}: bits requires an unsigned integer type \
                         (u8, u16be, u16le, u32be, u32le, u32me), got {name}"
                    ),
                ))
            }
            Some(carrier) => check_bits(bits, carrier, &format!("type {name}"), path, ctx)?,
        }
    }
    if ty == FieldType::AsciiInt {
        let len =
            f.len.ok_or_else(|| ferr(path, format!("{ctx}: type ascii_int requires len")))?;
        if len == 0 {
            return Err(ferr(path, format!("{ctx}: len must be >= 1")));
        }
        Ok(len)
    } else {
        if f.len.is_some() {
            return Err(ferr(path, format!("{ctx}: len is only valid for type ascii_int")));
        }
        ty.size()
            .ok_or_else(|| ferr(path, format!("{ctx}: text-only type not usable for binary decode")))
    }
}

// ---------------------------------------------------------------------------
// Workflow
// ---------------------------------------------------------------------------

/// A workflow is a fixed sequence of command invocations plus a `finally`
/// block — deliberately not a programming language. Whether the referenced
/// commands exist is a device-level check (core does not know the device
/// directory); use `Workflow::referenced_commands` for it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    /// Format identifier; must be "openbaud/workflow@v0".
    pub schema: String,
    /// Workflow name; used as the file name in the device directory.
    pub name: String,
    /// Free-form description of what the workflow does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Commands run in order; a failing step aborts the sequence.
    pub steps: Vec<StepSpec>,
    /// Commands always run afterwards, even when a step failed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finally: Vec<StepSpec>,
}

/// One workflow step: a command invocation with optional parameter values.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    /// Name of the command to run (must exist in the device directory).
    pub command: String,
    /// Parameter values passed to the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Workflow {
    /// Names of every command referenced by steps and finally, in order of
    /// first appearance, deduplicated. Intended for device-level existence
    /// checks.
    pub fn referenced_commands(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for step in self.steps.iter().chain(self.finally.iter()) {
            if !seen.contains(&step.command) {
                seen.push(step.command.clone());
            }
        }
        seen
    }
}

pub fn parse_workflow(yaml: &str, path: &str) -> crate::Result<Workflow> {
    let wf: Workflow = serde_yaml::from_str(yaml).map_err(|e| ferr(path, e.to_string()))?;
    if wf.schema != WORKFLOW_SCHEMA {
        return Err(ferr(path, format!("schema is {:?}, expected {WORKFLOW_SCHEMA:?}", wf.schema)));
    }
    if wf.name.is_empty() {
        return Err(ferr(path, "name must not be empty"));
    }
    if wf.steps.is_empty() {
        return Err(ferr(path, "steps must not be empty"));
    }
    for (i, step) in wf.steps.iter().chain(wf.finally.iter()).enumerate() {
        if step.command.is_empty() {
            return Err(ferr(path, format!("step {} has an empty command name", i + 1)));
        }
    }
    Ok(wf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = r#"
schema: openbaud/profile@v0
name: pzem004t
description: PZEM-004T v3 power meter
transport: { baud: 9600 }
framing: { idle_ms: 30 }
"#;

    const COMMAND: &str = r#"
schema: openbaud/command@v0
name: read_voltage
risk: read
params:
  - { name: addr, type: u8, default: 1, min: 1, max: 247 }
frame:
  hex: "{addr} 04 00 00 00 01 {crc16_modbus}"
response:
  match: { length: 7 }
  validate: { checksum: crc16_modbus }
  parse:
    fields:
      voltage: { at: 3, type: u16be, scale: 0.1, unit: "V" }
provenance:
  datasheet: "datasheet.pdf#page=7"
"#;

    #[test]
    fn parses_valid_profile_and_command() {
        let p = parse_profile(PROFILE, "profile.yaml").unwrap();
        assert_eq!(p.transport.baud, 9600);
        assert!(p.framing.unwrap().to_framing("profile.yaml").is_ok());

        let c = parse_command(COMMAND, "read_voltage.yaml").unwrap();
        assert_eq!(c.risk, Risk::Read);
        assert_eq!(c.response.as_ref().unwrap().timeout_ms, 3000);
    }

    const SCAN_COMMAND: &str = r#"
schema: openbaud/command@v0
name: scan
frame: { hex: "A5 20" }
response:
  match: { length: 20 }
  parse:
    fields:
      point_count: { at: 0, type: u8 }
      points:
        at: 1
        count: { field: point_count }
        stride: 5
        elements:
          bearing: { at: 0, type: u16le, scale: 0.01, unit: deg }
          range_mm: { at: 2, type: u16le, unit: mm }
          quality: { at: 4, type: u8 }
  view: { kind: polar, data: points, angle: bearing, radius: range_mm, intensity: quality }
"#;

    #[test]
    fn view_binds_channels_to_the_devices_own_field_names() {
        // The point of encodings: no field is required to be called angle_deg.
        let cmd = parse_command(SCAN_COMMAND, "scan.yaml").unwrap();
        let view = cmd.response.as_ref().unwrap().view.as_ref().expect("view parsed");
        assert_eq!(view.kind, ViewKind::Polar);
        assert_eq!(view.data, "points");
        assert_eq!(view.angle.as_deref(), Some("bearing"));
        assert_eq!(view.radius.as_deref(), Some("range_mm"));
        assert_eq!(view.intensity.as_deref(), Some("quality"));
    }

    #[test]
    fn view_channel_pointing_at_a_missing_field_is_loud() {
        let bad = SCAN_COMMAND.replace("angle: bearing", "angle: azimuth");
        let err = parse_command(&bad, "scan.yaml").unwrap_err().to_string();
        assert!(err.contains("azimuth"), "got: {err}");
        assert!(err.contains("points"), "error must name the record array: {err}");
    }

    #[test]
    fn view_data_must_name_a_record_array() {
        let bad = SCAN_COMMAND.replace("data: points", "data: point_count");
        let err = parse_command(&bad, "scan.yaml").unwrap_err().to_string();
        assert!(err.contains("point_count"), "got: {err}");
    }

    #[test]
    fn polar_view_requires_both_angle_and_radius() {
        let bad = SCAN_COMMAND.replace(", radius: range_mm", "");
        let err = parse_command(&bad, "scan.yaml").unwrap_err().to_string();
        assert!(err.contains("radius"), "got: {err}");
    }

    #[test]
    fn swapped_angle_and_radius_channels_are_caught_by_units() {
        // The failure this actually prevents: declaring the length field as
        // the angle. Units already carry the semantics — use them.
        let bad = SCAN_COMMAND
            .replace("angle: bearing, radius: range_mm", "angle: range_mm, radius: bearing");
        let err = parse_command(&bad, "scan.yaml").unwrap_err().to_string();
        assert!(err.contains("mm") || err.contains("unit"), "got: {err}");
    }

    #[test]
    fn tool_produced_view_kinds_are_rejected_in_yaml() {
        for kind in ["timeline", "diagnostics", "capture"] {
            let bad = SCAN_COMMAND.replace("kind: polar", &format!("kind: {kind}"));
            assert_ne!(bad, SCAN_COMMAND, "replacement did not apply");
            let err = parse_command(&bad, "scan.yaml").unwrap_err().to_string();
            assert!(err.contains("produced by tools"), "{kind}: {err}");
            assert!(
                err.contains("session_timeline")
                    && err.contains("diagnose_frame")
                    && err.contains("capture_frames"),
                "error must name the producing tools: {err}"
            );
            assert!(err.contains(kind), "error must name the rejected kind: {err}");
        }
    }

    #[test]
    fn view_kind_names_are_snake_case() {
        assert_eq!(serde_json::to_string(&ViewKind::Timeline).unwrap(), "\"timeline\"");
        assert_eq!(serde_json::to_string(&ViewKind::Diagnostics).unwrap(), "\"diagnostics\"");
        assert_eq!(serde_json::to_string(&ViewKind::Capture).unwrap(), "\"capture\"");
        assert_eq!(serde_json::from_str::<ViewKind>("\"polar\"").unwrap(), ViewKind::Polar);
        for (kind, name) in [
            (ViewKind::Polar, "polar"),
            (ViewKind::Timeline, "timeline"),
            (ViewKind::Diagnostics, "diagnostics"),
            (ViewKind::Capture, "capture"),
        ] {
            assert_eq!(kind.name(), name);
        }
    }

    #[test]
    fn units_reach_nested_record_elements() {
        // Element units are declared in YAML; they must survive into results
        // so a viewer can label axes without guessing.
        let cmd = parse_command(SCAN_COMMAND, "scan.yaml").unwrap();
        let units = crate::exec::units(&cmd);
        assert_eq!(units.get("points.bearing").and_then(|v| v.as_str()), Some("deg"));
        assert_eq!(units.get("points.range_mm").and_then(|v| v.as_str()), Some("mm"));
    }

    #[test]
    fn rejects_unknown_fields_with_path() {
        let bad = PROFILE.replace("description:", "descriptoin:");
        let err = parse_profile(&bad, "profile.yaml").unwrap_err();
        assert!(err.to_string().contains("profile.yaml"));
    }

    #[test]
    fn rejects_undeclared_template_param() {
        let bad = COMMAND.replace("{addr}", "{address}");
        let err = parse_command(&bad, "c.yaml").unwrap_err();
        assert!(err.to_string().contains("address"));
    }

    #[test]
    fn rejects_ambiguous_framing_and_match() {
        let bad = PROFILE.replace("{ idle_ms: 30 }", "{ idle_ms: 30, delimiter: \"\\n\" }");
        assert!(parse_profile(&bad, "p.yaml").is_err());

        let bad = COMMAND.replace("{ length: 7 }", "{ length: 7, idle_ms: 5 }");
        assert!(parse_command(&bad, "c.yaml").is_err());
    }

    #[test]
    fn rejects_bad_regex_and_types() {
        let cmd = r#"
schema: openbaud/command@v0
name: csq
frame: { text: "AT+CSQ\r\n" }
response:
  match: { delimiter: "\r\n" }
  parse: { regex: "([" }
"#;
        assert!(parse_command(cmd, "c.yaml").is_err());
    }

    #[test]
    fn old_format_defaults_for_new_fields() {
        let c = parse_command(COMMAND, "c.yaml").unwrap();
        let resp = c.response.as_ref().unwrap();
        assert_eq!(resp.expect, Expect::Normal);
        assert_eq!(resp.first_byte_ms, None);
        assert!(resp.exception.is_none());
        assert!(c.timing.is_none());
    }

    #[test]
    fn parses_timing_and_first_byte_ms_with_defaults() {
        let cmd = r#"
schema: openbaud/command@v0
name: read_voltage
frame: { hex: "01 04 00 00 00 01 {crc16_modbus}" }
timing: { pre_delay_ms: 100 }
response:
  match: { length: 7 }
  first_byte_ms: 500
"#;
        let c = parse_command(cmd, "c.yaml").unwrap();
        let timing = c.timing.unwrap();
        assert_eq!(timing.pre_delay_ms, 100);
        assert_eq!(timing.post_delay_ms, 0); // default
        assert_eq!(c.response.unwrap().first_byte_ms, Some(500));

        let bad = cmd.replace("first_byte_ms: 500", "first_byte_ms: 0");
        let err = parse_command(&bad, "c.yaml").unwrap_err();
        assert!(err.to_string().contains("first_byte_ms"));
    }

    const EXCEPTION_CMD: &str = r#"
schema: openbaud/command@v0
name: illegal_function_test
frame: { hex: "01 84 00 00 00 01 {crc16_modbus}" }
response:
  match: { length: 7 }
  expect: exception
  exception:
    when: { at: 1, equals: "84" }
    match: { length: 5 }
    validate: { checksum: crc16_modbus }
    parse:
      fields:
        function: { at: 1, type: u8 }
        exception_code: { at: 2, type: u8 }
"#;

    #[test]
    fn parses_exception_spec() {
        let c = parse_command(EXCEPTION_CMD, "c.yaml").unwrap();
        let resp = c.response.as_ref().unwrap();
        assert_eq!(resp.expect, Expect::Exception);
        let exc = resp.exception.as_ref().unwrap();
        assert_eq!(exc.when.at, 1);
        assert_eq!(exc.when.mask_byte().unwrap(), 0xFF); // default mask
        assert_eq!(exc.when.equals_byte().unwrap(), 0x84);
    }

    #[test]
    fn rejects_bad_exception_spec() {
        // mask must be a single hex byte
        let bad = EXCEPTION_CMD.replace("at: 1, equals", "at: 1, mask: \"FFFF\", equals");
        let err = parse_command(&bad, "c.yaml").unwrap_err();
        assert!(err.to_string().contains("exception.when.mask"));

        // equals must be valid hex
        let bad = EXCEPTION_CMD.replace("equals: \"84\"", "equals: \"zz\"");
        assert!(parse_command(&bad, "c.yaml").is_err());

        // exception.parse checksum name must be known
        let bad = EXCEPTION_CMD.replace("checksum: crc16_modbus", "checksum: crc999");
        let err = parse_command(&bad, "c.yaml").unwrap_err();
        assert!(err.to_string().contains("exception.validate"));

        // expect: exception without an exception block is unsatisfiable
        let cmd = r#"
schema: openbaud/command@v0
name: t
frame: { hex: "01" }
response:
  match: { length: 5 }
  expect: exception
"#;
        let err = parse_command(cmd, "c.yaml").unwrap_err();
        assert!(err.to_string().contains("exception"));
    }

    #[test]
    fn silence_allows_omitted_match_but_normal_requires_it() {
        let silent = r#"
schema: openbaud/command@v0
name: wrong_addr_test
frame: { hex: "F7 04 00 00 00 01 {crc16_modbus}" }
response:
  expect: silence
  timeout_ms: 500
"#;
        let c = parse_command(silent, "c.yaml").unwrap();
        let resp = c.response.as_ref().unwrap();
        assert_eq!(resp.expect, Expect::Silence);
        assert!(resp.match_spec.is_none());

        let bad = silent.replace("expect: silence", "expect: normal");
        let err = parse_command(&bad, "c.yaml").unwrap_err();
        assert!(err.to_string().contains("response.match is required"));

        // Default expect (normal) also requires match.
        let bad = silent.replace("  expect: silence\n", "");
        assert!(parse_command(&bad, "c.yaml").is_err());
    }

    const ARRAY_CMD: &str = r#"
schema: openbaud/command@v0
name: read_block
frame: { hex: "01" }
response:
  match: { idle_ms: 20 }
  validate: { checksum: sum16be, range: { from: 2, to: 7 }, at: -2, encoding: ascii_hex }
  parse:
    fields:
      n_points: { at: 0, type: ascii_int, len: 4 }
      samples: { at: 4, type: u8, count: { field: n_points }, scale: 0.5 }
      points:
        at: 4
        count: 3
        stride: 5
        elements:
          quality: { at: 0, type: u8 }
          dist: { at: 1, type: u16le, scale: 0.25 }
"#;

    #[test]
    fn parses_arrays_and_extended_validate() {
        let c = parse_command(ARRAY_CMD, "c.yaml").unwrap();
        let parse = c.response.as_ref().unwrap().parse.as_ref().unwrap();
        let fields = parse.fields.as_ref().unwrap();
        assert!(matches!(fields["points"].count, Some(CountSpec::Fixed(3))));
        assert!(matches!(&fields["samples"].count, Some(CountSpec::Field(r)) if r.field == "n_points"));
        let v = c.response.as_ref().unwrap().validate.as_ref().unwrap();
        assert_eq!(v.at, Some(-2));
        assert_eq!(v.encoding, Encoding::AsciiHex);
        assert_eq!((v.range.unwrap().from, v.range.unwrap().to), (2, 7));
    }

    #[test]
    fn rejects_bad_array_specs() {
        for (from, to, needle) in [
            // type and elements are mutually exclusive
            ("count: 3\n        stride: 5", "count: 3\n        stride: 5\n        type: u8", "mutually exclusive"),
            // ascii_int requires len
            ("type: ascii_int, len: 4", "type: ascii_int", "requires len"),
            // len only valid for ascii_int
            ("type: u8, count: { field: n_points }", "type: u8, len: 2, count: { field: n_points }", "only valid for type ascii_int"),
            // count field must exist
            ("count: { field: n_points }", "count: { field: nope }", "not declared"),
            // count field must be scalar
            ("count: { field: n_points }", "count: { field: points }", "must be a scalar field"),
            // fixed count must be >= 1
            ("count: 3", "count: 0", "count must be >= 1"),
            // record arrays need stride
            ("stride: 5", "", "stride is required with elements"),
            // element offset must fit the stride
            ("dist: { at: 1, type: u16le", "dist: { at: 4, type: u16le", "exceeds the record stride"),
            // one-level arrays only
            ("quality: { at: 0, type: u8 }", "quality: { at: 0, type: u8, count: 2 }", "nest only one level"),
            // stride requires count
            ("type: u8, count: { field: n_points }, scale: 0.5", "type: u8, stride: 2", "stride requires count"),
            // static range inversion (both ends non-negative)
            ("range: { from: 2, to: 7 }", "range: { from: 7, to: 2 }", "is after"),
        ] {
            let bad = ARRAY_CMD.replace(from, to);
            assert_ne!(bad, ARRAY_CMD, "replacement {from:?} did not apply");
            let err = parse_command(&bad, "c.yaml").unwrap_err();
            assert!(err.to_string().contains(needle), "expected {needle:?} in: {err}");
        }
    }

    #[test]
    fn rejects_ascii_int_param_and_stride_smaller_than_width() {
        let cmd = r#"
schema: openbaud/command@v0
name: t
params:
  - { name: n, type: ascii_int }
frame: { text: "{n}" }
"#;
        let err = parse_command(cmd, "c.yaml").unwrap_err();
        assert!(err.to_string().contains("not a param type"), "{err}");

        let bad = ARRAY_CMD.replace(
            "samples: { at: 4, type: u8, count: { field: n_points }, scale: 0.5 }",
            "samples: { at: 4, type: u16le, count: { field: n_points }, stride: 1 }",
        );
        let err = parse_command(&bad, "c.yaml").unwrap_err();
        assert!(err.to_string().contains("smaller than the element width"), "{err}");
    }

    const SPLIT_CMD: &str = r#"
schema: openbaud/command@v0
name: read_trace
frame: { text: "TRAC?\n" }
response:
  match: { delimiter: "\n" }
  parse:
    regex: 'F=(?P<flags>\S+) V=(?P<values>.*)$'
    types: { flags: hex_int }
    split:
      values: { sep: ",", type: float }
"#;

    #[test]
    fn parses_split_and_hex_int_types() {
        let c = parse_command(SPLIT_CMD, "c.yaml").unwrap();
        let parse = c.response.as_ref().unwrap().parse.as_ref().unwrap();
        let split = parse.split.as_ref().unwrap();
        assert_eq!(split["values"].sep, ",");
        assert!(matches!(&split["values"].type_spec, TextType::Name(n) if n == "float"));
    }

    #[test]
    fn rejects_bad_split_specs() {
        for (from, to, needle) in [
            // split key must be a named capture
            ("values: { sep: \",\", type: float }", "nope: { sep: \",\", type: float }", "no named capture"),
            // split element type restricted
            ("type: float }", "type: bytes }", "not one of int, float, string, hex_int"),
            // separator must not be empty
            ("sep: \",\"", "sep: \"\"", "must not be empty"),
            // a capture cannot be in both types and split
            ("types: { flags: hex_int }", "types: { values: string }", "both types and split"),
            // hex_int is fine, other unknown type names are not
            ("types: { flags: hex_int }", "types: { flags: hexint }", "not one of int, float, string, hex_int"),
        ] {
            let bad = SPLIT_CMD.replace(from, to);
            assert_ne!(bad, SPLIT_CMD, "replacement {from:?} did not apply");
            let err = parse_command(&bad, "c.yaml").unwrap_err();
            assert!(err.to_string().contains(needle), "expected {needle:?} in: {err}");
        }

        // split only applies to regex parsing
        let bad = ARRAY_CMD.replace(
            "    fields:",
            "    split:\n      x: { sep: \",\", type: int }\n    fields:",
        );
        let err = parse_command(&bad, "c.yaml").unwrap_err();
        assert!(err.to_string().contains("only applies to regex"), "{err}");
    }

    const BITS_CMD: &str = r#"
schema: openbaud/command@v0
name: lidar_node
frame: { hex: "A5 20" }
response:
  match: { length: 7 }
  parse:
    fields:
      temp_c: { at: 6, type: u8, offset: -40 }
      points:
        at: 1
        count: 1
        stride: 5
        elements:
          start_flag: { at: 0, type: u8,    bits: { lsb: 0, width: 1 } }
          quality:    { at: 0, type: u8,    bits: { lsb: 2, width: 6 } }
          angle_deg:  { at: 1, type: u16le, bits: { lsb: 1, width: 15 }, scale: 0.015625 }
"#;

    #[test]
    fn parses_bits_and_offset() {
        let c = parse_command(BITS_CMD, "c.yaml").unwrap();
        let parse = c.response.as_ref().unwrap().parse.as_ref().unwrap();
        let fields = parse.fields.as_ref().unwrap();
        assert_eq!(fields["temp_c"].offset, Some(-40.0));
        let elements = fields["points"].elements.as_ref().unwrap();
        assert_eq!(elements["quality"].bits, Some(BitsSpec { lsb: 2, width: 6 }));
        assert_eq!(elements["angle_deg"].bits, Some(BitsSpec { lsb: 1, width: 15 }));
    }

    #[test]
    fn rejects_bad_bits_and_offset_specs() {
        for (from, to, needle) in [
            // bits only on unsigned integer types: not signed ...
            ("start_flag: { at: 0, type: u8, ", "start_flag: { at: 0, type: i8, ", "unsigned integer type"),
            // ... not float ...
            ("angle_deg:  { at: 1, type: u16le,", "angle_deg:  { at: 3, type: f32le,", "unsigned integer type"),
            // ... not ascii_int
            ("temp_c: { at: 6, type: u8, offset: -40 }",
             "temp_c: { at: 0, type: ascii_int, len: 2, bits: { lsb: 0, width: 4 } }",
             "unsigned integer type"),
            // width must be non-zero
            ("bits: { lsb: 0, width: 1 }", "bits: { lsb: 0, width: 0 }", "width must be >= 1"),
            // lsb + width must fit the type's bit width
            ("bits: { lsb: 2, width: 6 }", "bits: { lsb: 3, width: 6 }", "exceeds the 8-bit type u8"),
            ("bits: { lsb: 1, width: 15 }", "bits: { lsb: 2, width: 15 }", "exceeds the 16-bit type u16le"),
            // the array container may not carry offset/bits (element fields do)
            ("count: 1\n        stride: 5", "count: 1\n        stride: 5\n        offset: -1", "belong on the element fields"),
            ("count: 1\n        stride: 5", "count: 1\n        stride: 5\n        bits: { lsb: 0, width: 1 }", "belong on the element fields"),
        ] {
            let bad = BITS_CMD.replace(from, to);
            assert_ne!(bad, BITS_CMD, "replacement {from:?} did not apply");
            let err = parse_command(&bad, "c.yaml").unwrap_err();
            assert!(err.to_string().contains(needle), "expected {needle:?} in: {err}");
        }
    }

    const TEXT_OBJ_CMD: &str = r#"
schema: openbaud/command@v0
name: obd_status
frame: { text: "0105\r" }
response:
  match: { delimiter: "\r" }
  parse:
    regex: '41 05 (?P<temp>[0-9A-Fa-f]+) (?P<flags>[0-9A-Fa-f]+) (?P<values>.*)$'
    types:
      temp:  { type: hex_int, offset: -40 }
      flags: { type: hex_int, bits: { lsb: 4, width: 4 } }
    split:
      values: { sep: ",", type: { type: int, scale: 0.5, offset: 1 } }
"#;

    #[test]
    fn parses_object_form_text_types() {
        let c = parse_command(TEXT_OBJ_CMD, "c.yaml").unwrap();
        let parse = c.response.as_ref().unwrap().parse.as_ref().unwrap();
        let types = parse.types.as_ref().unwrap();
        assert!(matches!(&types["temp"], TextType::Spec(s)
            if s.type_name == "hex_int" && s.offset == Some(-40.0) && s.bits.is_none()));
        assert!(matches!(&types["flags"], TextType::Spec(s)
            if s.bits == Some(BitsSpec { lsb: 4, width: 4 })));
        let split = parse.split.as_ref().unwrap();
        assert!(matches!(&split["values"].type_spec, TextType::Spec(s)
            if s.type_name == "int" && s.scale == Some(0.5) && s.offset == Some(1.0)));
    }

    #[test]
    fn rejects_bad_object_form_text_types() {
        for (from, to, needle) in [
            // string takes none of bits/scale/offset
            ("temp:  { type: hex_int, offset: -40 }",
             "temp:  { type: string, offset: -40 }",
             "do not apply to type string"),
            // float takes scale/offset but not bits
            ("flags: { type: hex_int, bits: { lsb: 4, width: 4 } }",
             "flags: { type: float, bits: { lsb: 4, width: 4 } }",
             "bits requires an integer type"),
            // shared bits sanity checks apply on the text side too
            ("bits: { lsb: 4, width: 4 }", "bits: { lsb: 4, width: 0 }", "width must be >= 1"),
            ("bits: { lsb: 4, width: 4 }", "bits: { lsb: 60, width: 10 }", "exceeds the 63-bit text integer"),
            // the base type name is still checked in object form
            ("{ type: int, scale: 0.5, offset: 1 }",
             "{ type: bytes, scale: 0.5, offset: 1 }",
             "not one of int, float, string, hex_int"),
        ] {
            let bad = TEXT_OBJ_CMD.replace(from, to);
            assert_ne!(bad, TEXT_OBJ_CMD, "replacement {from:?} did not apply");
            let err = parse_command(&bad, "c.yaml").unwrap_err();
            assert!(err.to_string().contains(needle), "expected {needle:?} in: {err}");
        }
    }

    const WORKFLOW: &str = r#"
schema: openbaud/workflow@v0
name: pzem_full_check
description: modbus round trip with recovery
steps:
  - command: set_mode_modbus
  - command: read_voltage
    params: { addr: 1 }
  - command: illegal_function_test
finally:
  - command: escape_binary
  - command: set_mode_echo
"#;

    #[test]
    fn parses_valid_workflow() {
        let wf = parse_workflow(WORKFLOW, "w.yaml").unwrap();
        assert_eq!(wf.name, "pzem_full_check");
        assert_eq!(wf.steps.len(), 3);
        assert_eq!(wf.finally.len(), 2);
        assert_eq!(
            wf.steps[1].params.as_ref().unwrap().get("addr"),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            wf.referenced_commands(),
            vec![
                "set_mode_modbus",
                "read_voltage",
                "illegal_function_test",
                "escape_binary",
                "set_mode_echo"
            ]
        );
    }

    #[test]
    fn workflow_finally_defaults_empty() {
        let minimal = r#"
schema: openbaud/workflow@v0
name: single
steps:
  - command: read_voltage
"#;
        let wf = parse_workflow(minimal, "w.yaml").unwrap();
        assert!(wf.finally.is_empty());
        assert_eq!(wf.referenced_commands(), vec!["read_voltage"]);
    }

    #[test]
    fn rejects_bad_workflows() {
        let empty_steps = r#"
schema: openbaud/workflow@v0
name: nothing
steps: []
"#;
        let err = parse_workflow(empty_steps, "w.yaml").unwrap_err();
        assert!(err.to_string().contains("steps must not be empty"));

        let unknown_field = WORKFLOW.replace("description:", "descriptoin:");
        let err = parse_workflow(&unknown_field, "w.yaml").unwrap_err();
        assert!(err.to_string().contains("w.yaml"));

        let wrong_schema = WORKFLOW.replace("openbaud/workflow@v0", "openbaud/command@v0");
        assert!(parse_workflow(&wrong_schema, "w.yaml").is_err());
    }
}
