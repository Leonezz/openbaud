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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub transport: Transport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<FramingSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<SelectorSpec>,
}

/// Stable device identity for automatic port resolution: all present fields
/// must match (AND). vid/pid are hex, case-insensitive; product is a
/// substring match; serial_number is exact.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transport {
    #[serde(default = "default_baud")]
    pub baud: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default)]
    pub parity: Parity,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Parity {
    #[default]
    None,
    Even,
    Odd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FramingSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_prefix: Option<LengthPrefixSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LengthPrefixSpec {
    pub header_len: usize,
    pub len_at: usize,
    pub len_size: usize,
    #[serde(default)]
    pub endian: Endian,
    #[serde(default)]
    pub extra: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub schema: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub risk: Risk,
    #[serde(default)]
    pub params: Vec<ParamSpec>,
    pub frame: FrameSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<TimingSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

/// Optional command-level timing: delay before sending and quiet period after
/// the command completes. Absent field == 0.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingSpec {
    #[serde(default)]
    pub pre_delay_ms: u64,
    #[serde(default)]
    pub post_delay_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    #[default]
    Read,
    Write,
    Danger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validate: Option<ValidateSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse: Option<ParseSpec>,
    /// Recognition and decoding of protocol-level exception frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exception: Option<ExceptionSpec>,
}

fn default_timeout_ms() -> u64 {
    3000
}

/// Declared success outcome for a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateSpec {
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, FieldSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    pub at: usize,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datasheet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<Verified>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verified {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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
            ChecksumKind::from_name(&v.checksum).map_err(|e| ferr(path, format!("validate: {e}")))?;
        }
        if let Some(parse) = &resp.parse {
            check_parse_spec(parse, path, "parse")?;
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
                ChecksumKind::from_name(&v.checksum)
                    .map_err(|e| ferr(path, format!("exception.validate: {e}")))?;
            }
            if let Some(parse) = &exc.parse {
                check_parse_spec(parse, path, "exception.parse")?;
            }
        }
    }
    Ok(cmd)
}

/// Shared semantic checks for a `parse` block (main response and exception).
/// `ctx` names the block in error messages ("parse" or "exception.parse").
fn check_parse_spec(parse: &ParseSpec, path: &str, ctx: &str) -> crate::Result<()> {
    match (&parse.fields, &parse.regex) {
        (Some(_), Some(_)) => {
            return Err(ferr(path, format!("{ctx} must set fields or regex, not both")))
        }
        (None, None) => return Err(ferr(path, format!("{ctx} must set one of: fields, regex"))),
        (Some(fields), None) => {
            for (fname, f) in fields {
                let ty = FieldType::from_name(&f.type_name)
                    .map_err(|e| ferr(path, format!("{ctx}.fields.{fname}: {e}")))?;
                if ty.size().is_none() {
                    return Err(ferr(
                        path,
                        format!("{ctx}.fields.{fname}: text-only type not usable for binary decode"),
                    ));
                }
            }
        }
        (None, Some(re)) => {
            regex::Regex::new(re)
                .map_err(|e| ferr(path, format!("{ctx}.regex does not compile: {e}")))?;
        }
    }
    if let Some(types) = &parse.types {
        for (name, t) in types {
            if !matches!(t.as_str(), "int" | "float" | "string") {
                return Err(ferr(
                    path,
                    format!("{ctx}.types.{name}: {t:?} is not one of int, float, string"),
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Workflow
// ---------------------------------------------------------------------------

/// A workflow is a fixed sequence of command invocations plus a `finally`
/// block — deliberately not a programming language. Whether the referenced
/// commands exist is a device-level check (core does not know the device
/// directory); use `Workflow::referenced_commands` for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub schema: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub steps: Vec<StepSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finally: Vec<StepSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    pub command: String,
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
        let bad = EXCEPTION_CMD.replace("checksum: crc16_modbus", "checksum: crc32");
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
