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
    pub provenance: Option<Provenance>,
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
    #[serde(rename = "match")]
    pub match_spec: MatchSpec,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validate: Option<ValidateSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse: Option<ParseSpec>,
}

fn default_timeout_ms() -> u64 {
    3000
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
        resp.match_spec.to_rule(path)?;
        if let Some(v) = &resp.validate {
            ChecksumKind::from_name(&v.checksum).map_err(|e| ferr(path, format!("validate: {e}")))?;
        }
        if let Some(parse) = &resp.parse {
            match (&parse.fields, &parse.regex) {
                (Some(_), Some(_)) => {
                    return Err(ferr(path, "parse must set fields or regex, not both"))
                }
                (None, None) => {
                    return Err(ferr(path, "parse must set one of: fields, regex"))
                }
                (Some(fields), None) => {
                    for (fname, f) in fields {
                        let ty = FieldType::from_name(&f.type_name)
                            .map_err(|e| ferr(path, format!("parse.fields.{fname}: {e}")))?;
                        if ty.size().is_none() {
                            return Err(ferr(
                                path,
                                format!("parse.fields.{fname}: text-only type not usable for binary decode"),
                            ));
                        }
                    }
                }
                (None, Some(re)) => {
                    regex::Regex::new(re)
                        .map_err(|e| ferr(path, format!("parse.regex does not compile: {e}")))?;
                }
            }
            if let Some(types) = &parse.types {
                for (name, t) in types {
                    if !matches!(t.as_str(), "int" | "float" | "string") {
                        return Err(ferr(
                            path,
                            format!("parse.types.{name}: {t:?} is not one of int, float, string"),
                        ));
                    }
                }
            }
        }
    }
    Ok(cmd)
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
}
