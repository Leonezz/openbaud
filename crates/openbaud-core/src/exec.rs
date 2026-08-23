//! Command execution semantics: parameter resolution, frame building and
//! response parsing. IO-free — the engine feeds raw bytes in and out.

use crate::checksum::ChecksumKind;
use crate::codec::{decode_ascii_int, FieldType};
use crate::format::{
    Command, CountSpec, Encoding, Expect, ExceptionSpec, FieldSpec, ParseSpec, ValidateSpec,
};
use crate::template::{render_text, HexTemplate};
use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Merge provided values with declared defaults; reject unknown names, missing
/// values and range violations.
pub fn resolve_params(cmd: &Command, provided: &Map<String, Value>) -> crate::Result<Map<String, Value>> {
    for key in provided.keys() {
        if !cmd.params.iter().any(|p| &p.name == key) {
            return Err(CoreError::Param {
                name: key.clone(),
                reason: format!("not declared by command {:?}", cmd.name),
            });
        }
    }
    let mut resolved = Map::new();
    for spec in &cmd.params {
        let value = provided
            .get(&spec.name)
            .cloned()
            .or_else(|| spec.default.clone())
            .ok_or_else(|| CoreError::Param {
                name: spec.name.clone(),
                reason: "no value provided and no default declared".to_string(),
            })?;
        if let Some(n) = value.as_f64() {
            if let Some(min) = spec.min {
                if n < min {
                    return Err(CoreError::Param {
                        name: spec.name.clone(),
                        reason: format!("{n} is below min {min}"),
                    });
                }
            }
            if let Some(max) = spec.max {
                if n > max {
                    return Err(CoreError::Param {
                        name: spec.name.clone(),
                        reason: format!("{n} is above max {max}"),
                    });
                }
            }
        }
        resolved.insert(spec.name.clone(), value);
    }
    Ok(resolved)
}

/// Build the on-wire frame for a command with the given parameter values.
pub fn build_frame(cmd: &Command, provided: &Map<String, Value>) -> crate::Result<Vec<u8>> {
    let values = resolve_params(cmd, provided)?;
    if let Some(hex) = &cmd.frame.hex {
        let template = HexTemplate::parse(hex)?;
        template.build(&values, &cmd.param_types())
    } else if let Some(text) = &cmd.frame.text {
        Ok(render_text(text, &values)?.into_bytes())
    } else {
        Err(CoreError::Template("command has neither hex nor text frame".to_string()))
    }
}

/// Validate and parse a raw response per the command's response spec. Returns
/// a flat JSON object of decoded values (empty object when the command
/// declares no parse rules).
pub fn parse_response(cmd: &Command, raw: &[u8]) -> crate::Result<Value> {
    let Some(resp) = &cmd.response else {
        return Ok(Value::Object(Map::new()));
    };
    if let Some(validate) = &resp.validate {
        verify_checksum(validate, raw)?;
    }
    let Some(parse) = &resp.parse else {
        return Ok(Value::Object(Map::new()));
    };
    parse_with_spec(parse, raw)
}

// ---------------------------------------------------------------------------
// Checksum verification per ValidateSpec
// ---------------------------------------------------------------------------

/// Resolve a possibly-negative frame index (-1 = last byte) to an absolute
/// one, rejecting out-of-range values loudly.
fn resolve_index(idx: i64, len: usize, what: &str) -> crate::Result<usize> {
    let abs = if idx < 0 { len as i64 + idx } else { idx };
    if abs < 0 || abs >= len as i64 {
        return Err(CoreError::Parse(format!(
            "{what} index {idx} is outside the {len}-byte frame"
        )));
    }
    Ok(abs as usize)
}

/// Verify a frame's checksum per a full `ValidateSpec`: explicit or default
/// computation range, checksum position (`at`, negative counts from the frame
/// end) and raw/ascii_hex value encoding.
pub fn verify_checksum(spec: &ValidateSpec, frame: &[u8]) -> crate::Result<()> {
    let kind = ChecksumKind::from_name(&spec.checksum)?;
    let n = kind.len();
    let stored = match spec.encoding {
        Encoding::Raw => n,
        Encoding::AsciiHex => 2 * n,
    };
    let len = frame.len();
    if len < stored {
        return Err(CoreError::Parse(format!(
            "frame of {len} bytes is too short to carry a {} checksum ({stored} byte(s))",
            kind.name()
        )));
    }
    let at = match spec.at {
        Some(i) => resolve_index(i, len, "validate.at")?,
        None => len - stored,
    };
    if at + stored > len {
        return Err(CoreError::Parse(format!(
            "checksum at byte {at} ({stored} byte(s)) exceeds the {len}-byte frame"
        )));
    }
    let (from, to) = match &spec.range {
        Some(r) => (
            resolve_index(r.from, len, "validate.range.from")?,
            resolve_index(r.to, len, "validate.range.to")?,
        ),
        None => {
            if at == 0 {
                return Err(CoreError::Parse(
                    "checksum sits at byte 0: no bytes precede it to compute over; set validate.range"
                        .to_string(),
                ));
            }
            (0, at - 1)
        }
    };
    if from > to {
        return Err(CoreError::Parse(format!(
            "validate.range resolves to an inverted interval [{from}, {to}] in the {len}-byte frame"
        )));
    }
    let expected = kind.compute(&frame[from..=to]);
    match spec.encoding {
        Encoding::Raw => {
            let actual = &frame[at..at + n];
            if actual != expected {
                return Err(CoreError::ChecksumMismatch {
                    expected: crate::hex::to_hex(&expected),
                    actual: crate::hex::to_hex(actual),
                    at,
                });
            }
        }
        Encoding::AsciiHex => {
            let chars = &frame[at..at + stored];
            let text = std::str::from_utf8(chars).map_err(|_| {
                CoreError::Parse(format!(
                    "ascii_hex checksum at byte {at} is not ASCII text: {}",
                    crate::hex::to_hex(chars)
                ))
            })?;
            let mut actual = Vec::with_capacity(n);
            for i in 0..n {
                let pair = &text[2 * i..2 * i + 2];
                actual.push(u8::from_str_radix(pair, 16).map_err(|_| {
                    CoreError::Parse(format!(
                        "ascii_hex checksum at byte {at}: {pair:?} is not a hex byte"
                    ))
                })?);
            }
            if actual != expected {
                return Err(CoreError::ChecksumMismatch {
                    expected: crate::hex::to_hex(&expected),
                    actual: format!("{text:?} ({})", crate::hex::to_hex(&actual)),
                    at,
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

fn parse_with_spec(parse: &ParseSpec, raw: &[u8]) -> crate::Result<Value> {
    let mut out = Map::new();
    if let Some(fields) = &parse.fields {
        // Scalars first, so field-driven array counts can reference them.
        for (name, field) in fields.iter().filter(|(_, f)| f.count.is_none()) {
            out.insert(name.clone(), decode_scalar_at(field, raw, field.at, name)?);
        }
        for (name, field) in fields.iter().filter(|(_, f)| f.count.is_some()) {
            out.insert(name.clone(), decode_array(name, field, raw, &out)?);
        }
    } else if let Some(re) = &parse.regex {
        let text = String::from_utf8_lossy(raw);
        let regex = regex::Regex::new(re)
            .map_err(|e| CoreError::Parse(format!("regex does not compile: {e}")))?;
        let caps = regex.captures(&text).ok_or_else(|| {
            CoreError::Parse(format!("regex {re:?} did not match response {text:?}"))
        })?;
        for name in regex.capture_names().flatten() {
            let Some(m) = caps.name(name) else { continue };
            if let Some(split) = parse.split.as_ref().and_then(|s| s.get(name)) {
                let mut arr = Vec::new();
                for segment in m.as_str().split(split.sep.as_str()) {
                    let segment = segment.trim();
                    if segment.is_empty() {
                        continue; // e.g. trailing separators
                    }
                    let value = coerce_text(&split.type_name, segment).map_err(|e| {
                        CoreError::Parse(format!(
                            "capture {name:?} element {}: {e}",
                            arr.len() + 1
                        ))
                    })?;
                    arr.push(value);
                }
                out.insert(name.to_string(), Value::Array(arr));
            } else {
                let coerce = parse
                    .types
                    .as_ref()
                    .and_then(|t| t.get(name))
                    .map(String::as_str)
                    .unwrap_or("string");
                let value = coerce_text(coerce, m.as_str())
                    .map_err(|e| CoreError::Parse(format!("capture {name:?}: {e}")))?;
                out.insert(name.to_string(), value);
            }
        }
    }
    Ok(Value::Object(out))
}

/// Convert one text token per a declared coercion type. The error message
/// carries the offending token; callers add the capture/element context.
fn coerce_text(coerce: &str, s: &str) -> crate::Result<Value> {
    match coerce {
        "int" => Ok(Value::from(
            s.parse::<i64>()
                .map_err(|_| CoreError::Parse(format!("{s:?} is not an int")))?,
        )),
        "float" => Ok(Value::from(
            s.parse::<f64>()
                .map_err(|_| CoreError::Parse(format!("{s:?} is not a float")))?,
        )),
        "hex_int" => Ok(Value::from(parse_hex_int(s)?)),
        _ => Ok(Value::from(s)),
    }
}

/// Parse a hex integer token: optional 0x/0X prefix or bare hex digits,
/// case-insensitive. Empty or invalid input is a loud error.
fn parse_hex_int(s: &str) -> crate::Result<i64> {
    let digits = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if digits.is_empty() {
        return Err(CoreError::Parse(format!("{s:?} is not a hex integer")));
    }
    i64::from_str_radix(digits, 16)
        .map_err(|_| CoreError::Parse(format!("{s:?} is not a hex integer")))
}

/// Decode one scalar occurrence of `field` at absolute offset `at`, applying
/// `scale`. `name` is only for error context.
fn decode_scalar_at(field: &FieldSpec, raw: &[u8], at: usize, name: &str) -> crate::Result<Value> {
    let type_name = field
        .type_name
        .as_deref()
        .ok_or_else(|| CoreError::Parse(format!("field {name:?} has no scalar type")))?;
    let ty = FieldType::from_name(type_name)?;
    let value = if ty == FieldType::AsciiInt {
        let len = field
            .len
            .ok_or_else(|| CoreError::Parse(format!("field {name:?}: ascii_int requires len")))?;
        decode_ascii_int(raw, at, len)?
    } else {
        ty.decode(raw, at)?
    };
    Ok(match field.scale {
        Some(scale) => {
            let n = value.as_f64().expect("binary decode always yields a number");
            Value::from(round10(n * scale))
        }
        None => value,
    })
}

/// Byte width of one scalar occurrence: the type's size, or `len` for
/// ascii_int.
fn scalar_width(field: &FieldSpec, name: &str) -> crate::Result<usize> {
    let type_name = field
        .type_name
        .as_deref()
        .ok_or_else(|| CoreError::Parse(format!("field {name:?} has no scalar type")))?;
    let ty = FieldType::from_name(type_name)?;
    if ty == FieldType::AsciiInt {
        field
            .len
            .ok_or_else(|| CoreError::Parse(format!("field {name:?}: ascii_int requires len")))
    } else {
        ty.size().ok_or_else(|| {
            CoreError::Parse(format!("field {name:?}: text-only type not usable for binary decode"))
        })
    }
}

/// Resolve an array's element count: a fixed positive integer, or the value
/// of an already-decoded scalar field (0 yields an empty array; negative is a
/// loud error).
fn resolve_count(name: &str, spec: &CountSpec, scalars: &Map<String, Value>) -> crate::Result<usize> {
    let n = match spec {
        CountSpec::Fixed(n) => *n,
        CountSpec::Field(r) => scalars
            .get(&r.field)
            .ok_or_else(|| {
                CoreError::Parse(format!(
                    "array {name:?}: count field {:?} was not decoded as a scalar",
                    r.field
                ))
            })?
            .as_i64()
            .ok_or_else(|| {
                CoreError::Parse(format!(
                    "array {name:?}: count field {:?} did not decode to an integer",
                    r.field
                ))
            })?,
    };
    usize::try_from(n).map_err(|_| {
        CoreError::Parse(format!("array {name:?}: count resolved to negative value {n}"))
    })
}

/// Decode an array field (scalar or record array) with bounds checking.
fn decode_array(
    name: &str,
    field: &FieldSpec,
    raw: &[u8],
    scalars: &Map<String, Value>,
) -> crate::Result<Value> {
    let count_spec = field.count.as_ref().expect("caller filtered on count");
    let count = resolve_count(name, count_spec, scalars)?;
    let (stride, elements): (usize, Option<&BTreeMap<String, FieldSpec>>) = match &field.elements {
        Some(elements) => {
            let stride = field.stride.ok_or_else(|| {
                CoreError::Parse(format!("array {name:?}: stride is required with elements"))
            })?;
            (stride, Some(elements))
        }
        None => (
            match field.stride {
                Some(s) => s,
                None => scalar_width(field, name)?,
            },
            None,
        ),
    };
    let end = count
        .checked_mul(stride)
        .and_then(|span| span.checked_add(field.at))
        .ok_or_else(|| CoreError::Parse(format!("array {name:?}: size overflows")))?;
    if end > raw.len() {
        return Err(CoreError::Parse(format!(
            "array {name:?}: at {} + count {count} x stride {stride} = {end} exceeds response length {}",
            field.at,
            raw.len()
        )));
    }
    let mut arr = Vec::with_capacity(count);
    for i in 0..count {
        let base = field.at + i * stride;
        match elements {
            None => arr.push(decode_scalar_at(field, raw, base, name)?),
            Some(elements) => {
                let mut record = Map::new();
                for (ename, e) in elements {
                    let value = decode_scalar_at(e, raw, base + e.at, ename).map_err(|err| {
                        CoreError::Parse(format!("array {name:?} record {i}: {err}"))
                    })?;
                    record.insert(ename.clone(), value);
                }
                arr.push(Value::Object(record));
            }
        }
    }
    Ok(Value::Array(arr))
}

// ---------------------------------------------------------------------------
// Outcome classification
// ---------------------------------------------------------------------------

/// What the engine observed on the wire for one request. The engine produces
/// this; `classify` refines a completed `Frame` into checksum/parse outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawOutcome {
    /// Zero bytes received by the deadline (or no first byte within
    /// `first_byte_ms`).
    Silence,
    /// Some bytes arrived but the match rule was never satisfied.
    Timeout { partial: Vec<u8> },
    /// A complete frame per the (main or exception) match rule.
    Frame { bytes: Vec<u8>, is_exception: bool },
}

/// The six-way classification of a command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Silence,
    Timeout,
    ChecksumError,
    Malformed,
    Exception,
    Normal,
}

/// Classification result: the outcome plus whatever material supports it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedResponse {
    pub outcome: Outcome,
    /// Decoded values of a `normal` frame.
    pub parsed: Option<Value>,
    /// Decoded values of an `exception` frame.
    pub exception: Option<Value>,
    /// The complete frame, when one was received.
    pub raw: Option<Vec<u8>>,
    /// Partial bytes, only on `timeout`.
    pub partial: Option<Vec<u8>>,
    /// Human-readable reason for checksum_error / malformed / timeout.
    pub detail: Option<String>,
}

impl ClassifiedResponse {
    fn bare(outcome: Outcome) -> Self {
        Self { outcome, parsed: None, exception: None, raw: None, partial: None, detail: None }
    }
}

/// Classify a raw engine outcome per the command's response declaration.
/// Commands without a `response` block do not go through this path.
pub fn classify(cmd: &Command, raw: RawOutcome) -> ClassifiedResponse {
    match raw {
        RawOutcome::Silence => ClassifiedResponse::bare(Outcome::Silence),
        RawOutcome::Timeout { partial } => ClassifiedResponse {
            detail: Some(format!(
                "received {} byte(s) but the match rule was not satisfied",
                partial.len()
            )),
            partial: Some(partial),
            ..ClassifiedResponse::bare(Outcome::Timeout)
        },
        RawOutcome::Frame { bytes, is_exception: false } => {
            let resp = cmd.response.as_ref();
            classify_frame(
                resp.and_then(|r| r.validate.as_ref()),
                resp.and_then(|r| r.parse.as_ref()),
                bytes,
                false,
            )
        }
        RawOutcome::Frame { bytes, is_exception: true } => {
            let Some(exc) = cmd.response.as_ref().and_then(|r| r.exception.as_ref()) else {
                return ClassifiedResponse {
                    detail: Some(
                        "frame flagged as exception but command declares no exception spec"
                            .to_string(),
                    ),
                    raw: Some(bytes),
                    ..ClassifiedResponse::bare(Outcome::Malformed)
                };
            };
            classify_frame(exc.validate.as_ref(), exc.parse.as_ref(), bytes, true)
        }
    }
}

fn classify_frame(
    validate: Option<&ValidateSpec>,
    parse: Option<&ParseSpec>,
    bytes: Vec<u8>,
    as_exception: bool,
) -> ClassifiedResponse {
    if let Some(v) = validate {
        if let Err(e) = verify_checksum(v, &bytes) {
            return ClassifiedResponse {
                detail: Some(e.to_string()),
                raw: Some(bytes),
                ..ClassifiedResponse::bare(Outcome::ChecksumError)
            };
        }
    }
    let decoded = match parse {
        Some(p) => match parse_with_spec(p, &bytes) {
            Ok(v) => v,
            Err(e) => {
                return ClassifiedResponse {
                    detail: Some(e.to_string()),
                    raw: Some(bytes),
                    ..ClassifiedResponse::bare(Outcome::Malformed)
                }
            }
        },
        None => Value::Object(Map::new()),
    };
    if as_exception {
        ClassifiedResponse {
            exception: Some(decoded),
            raw: Some(bytes),
            ..ClassifiedResponse::bare(Outcome::Exception)
        }
    } else {
        ClassifiedResponse {
            parsed: Some(decoded),
            raw: Some(bytes),
            ..ClassifiedResponse::bare(Outcome::Normal)
        }
    }
}

/// Whether the outcome satisfies the command's declared expectation.
/// Commands without a `response` block expect `normal`.
pub fn expect_met(cmd: &Command, outcome: &Outcome) -> bool {
    let expect = cmd.response.as_ref().map(|r| r.expect).unwrap_or_default();
    matches!(
        (expect, outcome),
        (Expect::Normal, Outcome::Normal)
            | (Expect::Exception, Outcome::Exception)
            | (Expect::Silence, Outcome::Silence)
    )
}

/// Prefix test for exception recognition: `(byte[at] & mask) == equals`.
/// Returns `None` while fewer than `at + 1` bytes have arrived (undecided),
/// `Some(hit)` once the byte is available.
///
/// Panics if the spec's mask/equals are not single hex bytes — impossible for
/// specs that went through `parse_command`.
pub fn exception_triggered(spec: &ExceptionSpec, bytes: &[u8]) -> Option<bool> {
    let byte = *bytes.get(spec.when.at)?;
    let mask = spec.when.mask_byte().expect("mask validated by parse_command");
    let equals = spec.when.equals_byte().expect("equals validated by parse_command");
    Some(byte & mask == equals)
}

/// Units declared by the command's parse fields, for display alongside values.
pub fn units(cmd: &Command) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(fields) = cmd.response.as_ref().and_then(|r| r.parse.as_ref()).and_then(|p| p.fields.as_ref())
    {
        for (name, field) in fields {
            if let Some(unit) = &field.unit {
                out.insert(name.clone(), Value::from(unit.clone()));
            }
        }
    }
    out
}

/// Round away f64 noise introduced by decimal scale factors (e.g. 2203 * 0.1).
fn round10(x: f64) -> f64 {
    (x * 1e10).round() / 1e10
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::ChecksumKind;
    use crate::format::parse_command;
    use serde_json::json;

    const MODBUS_CMD: &str = r#"
schema: openbaud/command@v0
name: read_voltage
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
"#;

    const MODBUS_U32ME_CMD: &str = r#"
schema: openbaud/command@v0
name: read_energy
params:
  - { name: addr, type: u8, default: 1, min: 1, max: 247 }
frame:
  hex: "{addr} 04 00 05 00 02 {crc16_modbus}"
response:
  match: { length: 9 }
  validate: { checksum: crc16_modbus }
  parse:
    fields:
      energy: { at: 3, type: u32me, scale: 0.001, unit: "kWh" }
"#;

    const AT_CMD: &str = r#"
schema: openbaud/command@v0
name: signal_quality
frame: { text: "AT+CSQ\r\n" }
response:
  match: { delimiter: "\r\nOK\r\n" }
  parse:
    regex: '\+CSQ: (?P<rssi>\d+),(?P<ber>\d+)'
    types: { rssi: int, ber: int }
"#;

    fn modbus_response(voltage_raw: u16) -> Vec<u8> {
        let mut body = vec![0x01, 0x04, 0x02];
        body.extend(voltage_raw.to_be_bytes());
        let crc = ChecksumKind::Crc16Modbus.compute(&body);
        body.extend(crc);
        body
    }

    #[test]
    fn end_to_end_modbus() {
        let cmd = parse_command(MODBUS_CMD, "c.yaml").unwrap();
        let frame = build_frame(&cmd, &Map::new()).unwrap();
        assert_eq!(frame[0], 0x01); // default addr

        let parsed = parse_response(&cmd, &modbus_response(2203)).unwrap();
        assert_eq!(parsed, json!({"voltage": 220.3}));
        assert_eq!(units(&cmd).get("voltage").unwrap(), "V");
    }

    #[test]
    fn end_to_end_modbus_u32me() {
        let cmd = parse_command(MODBUS_U32ME_CMD, "c.yaml").unwrap();
        let frame = build_frame(&cmd, &Map::new()).unwrap();
        assert_eq!(frame[0], 0x01); // default addr

        // Logical energy 2000 Wh (0x000007D0) arrives word-swapped: 07 D0 00 00.
        let mut body = vec![0x01, 0x04, 0x04, 0x07, 0xD0, 0x00, 0x00];
        let crc = ChecksumKind::Crc16Modbus.compute(&body);
        body.extend(crc);

        let parsed = parse_response(&cmd, &body).unwrap();
        assert_eq!(parsed, json!({"energy": 2.0}));
        assert_eq!(units(&cmd).get("energy").unwrap(), "kWh");
    }

    #[test]
    fn corrupted_response_fails_checksum() {
        let cmd = parse_command(MODBUS_CMD, "c.yaml").unwrap();
        let mut resp = modbus_response(2203);
        resp[3] ^= 0xFF;
        assert!(matches!(
            parse_response(&cmd, &resp).unwrap_err(),
            CoreError::ChecksumMismatch { .. }
        ));
    }

    #[test]
    fn param_range_and_unknown_rejected() {
        let cmd = parse_command(MODBUS_CMD, "c.yaml").unwrap();
        let mut p = Map::new();
        p.insert("addr".to_string(), json!(300));
        assert!(build_frame(&cmd, &p).is_err());

        let mut p = Map::new();
        p.insert("bogus".to_string(), json!(1));
        assert!(build_frame(&cmd, &p).is_err());
    }

    #[test]
    fn text_command_with_regex_parse() {
        let cmd = parse_command(AT_CMD, "c.yaml").unwrap();
        assert_eq!(build_frame(&cmd, &Map::new()).unwrap(), b"AT+CSQ\r\n".to_vec());

        let parsed = parse_response(&cmd, b"\r\n+CSQ: 24,99\r\n").unwrap();
        assert_eq!(parsed, json!({"rssi": 24, "ber": 99}));

        assert!(parse_response(&cmd, b"ERROR").is_err());
    }

    // -- outcome classification --

    const EXCEPTION_CMD: &str = r#"
schema: openbaud/command@v0
name: illegal_function_test
params:
  - { name: addr, type: u8, default: 1 }
frame:
  hex: "{addr} 04 00 00 00 01 {crc16_modbus}"
response:
  match: { length: 7 }
  expect: exception
  validate: { checksum: crc16_modbus }
  parse:
    fields:
      voltage: { at: 3, type: u16be, scale: 0.1 }
  exception:
    when: { at: 1, equals: "84" }
    match: { length: 5 }
    validate: { checksum: crc16_modbus }
    parse:
      fields:
        function: { at: 1, type: u8 }
        exception_code: { at: 2, type: u8 }
"#;

    const SILENCE_CMD: &str = r#"
schema: openbaud/command@v0
name: wrong_addr_test
frame: { hex: "F7 04 00 00 00 01 {crc16_modbus}" }
response:
  expect: silence
  timeout_ms: 500
"#;

    fn modbus_exception_frame(exception_code: u8) -> Vec<u8> {
        let mut body = vec![0x01, 0x84, exception_code];
        let crc = ChecksumKind::Crc16Modbus.compute(&body);
        body.extend(crc);
        body
    }

    #[test]
    fn classify_modbus_exception_frame() {
        let cmd = parse_command(EXCEPTION_CMD, "c.yaml").unwrap();
        let frame = modbus_exception_frame(1);
        assert_eq!(frame.len(), 5);

        let exc = cmd.response.as_ref().unwrap().exception.as_ref().unwrap();
        assert_eq!(exception_triggered(exc, &frame[..1]), None); // undecided
        assert_eq!(exception_triggered(exc, &frame[..2]), Some(true));
        assert_eq!(exception_triggered(exc, &[0x01, 0x04]), Some(false));

        let r = classify(&cmd, RawOutcome::Frame { bytes: frame.clone(), is_exception: true });
        assert_eq!(r.outcome, Outcome::Exception);
        assert_eq!(r.exception, Some(json!({"function": 0x84, "exception_code": 1})));
        assert_eq!(r.parsed, None);
        assert_eq!(r.raw, Some(frame));
        assert!(expect_met(&cmd, &r.outcome));
    }

    #[test]
    fn classify_silence_and_timeout() {
        let cmd = parse_command(MODBUS_CMD, "c.yaml").unwrap();

        let r = classify(&cmd, RawOutcome::Silence);
        assert_eq!(r.outcome, Outcome::Silence);
        assert!(r.raw.is_none() && r.partial.is_none());

        let r = classify(&cmd, RawOutcome::Timeout { partial: vec![0x01, 0x04] });
        assert_eq!(r.outcome, Outcome::Timeout);
        assert_eq!(r.partial, Some(vec![0x01, 0x04]));
        assert!(r.detail.is_some());
    }

    #[test]
    fn classify_checksum_error_and_malformed() {
        let cmd = parse_command(MODBUS_CMD, "c.yaml").unwrap();
        let mut corrupted = modbus_response(2203);
        corrupted[3] ^= 0xFF;
        let r = classify(&cmd, RawOutcome::Frame { bytes: corrupted, is_exception: false });
        assert_eq!(r.outcome, Outcome::ChecksumError);
        assert!(r.detail.is_some());

        // Passes checksum but the regex does not match -> malformed.
        let at = parse_command(AT_CMD, "c.yaml").unwrap();
        let r = classify(&at, RawOutcome::Frame { bytes: b"ERROR".to_vec(), is_exception: false });
        assert_eq!(r.outcome, Outcome::Malformed);
        assert!(r.detail.is_some());
    }

    #[test]
    fn classify_normal_frame() {
        let cmd = parse_command(MODBUS_CMD, "c.yaml").unwrap();
        let frame = modbus_response(2203);
        let r = classify(&cmd, RawOutcome::Frame { bytes: frame.clone(), is_exception: false });
        assert_eq!(r.outcome, Outcome::Normal);
        assert_eq!(r.parsed, Some(json!({"voltage": 220.3})));
        assert_eq!(r.raw, Some(frame));
        assert!(expect_met(&cmd, &r.outcome));
    }

    #[test]
    fn expect_met_covers_all_declared_expectations() {
        let normal = parse_command(MODBUS_CMD, "c.yaml").unwrap();
        assert!(expect_met(&normal, &Outcome::Normal));
        assert!(!expect_met(&normal, &Outcome::Exception));
        assert!(!expect_met(&normal, &Outcome::Silence));

        let exception = parse_command(EXCEPTION_CMD, "c.yaml").unwrap();
        assert!(expect_met(&exception, &Outcome::Exception));
        assert!(!expect_met(&exception, &Outcome::Normal));

        let silence = parse_command(SILENCE_CMD, "c.yaml").unwrap();
        assert!(expect_met(&silence, &Outcome::Silence));
        assert!(!expect_met(&silence, &Outcome::Normal));
        assert!(!expect_met(&silence, &Outcome::Timeout));
    }

    // -- validate extensions: range / at / ascii_hex --

    const SDS011_CMD: &str = r#"
schema: openbaud/command@v0
name: sds011_read
frame: { hex: "AA B4 04 00 00 00 00 00 00 00 00 00 00 00 00 FF FF 05 AB" }
response:
  match: { length: 10 }
  validate:
    checksum: sum8
    range: { from: 2, to: 7 }
    at: -2
  parse:
    fields:
      pm25: { at: 2, type: u16le, scale: 0.1 }
"#;

    /// SDS011-style frame: AA C0 <6 data bytes> <sum8 of data> AB.
    fn sds011_frame(data: [u8; 6]) -> Vec<u8> {
        let sum = data.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        let mut f = vec![0xAA, 0xC0];
        f.extend(data);
        f.push(sum);
        f.push(0xAB);
        f
    }

    #[test]
    fn validate_subrange_with_tail_offset() {
        let cmd = parse_command(SDS011_CMD, "c.yaml").unwrap();
        let frame = sds011_frame([0xD4, 0x04, 0x3A, 0x0A, 0xA9, 0x60]);
        let parsed = parse_response(&cmd, &frame).unwrap();
        assert_eq!(parsed, json!({"pm25": 123.6})); // 0x04D4 * 0.1

        // Tampering a covered byte fails; the trailer byte AB is not covered.
        let mut bad = frame.clone();
        bad[3] ^= 0x01;
        let err = parse_response(&cmd, &bad).unwrap_err();
        assert!(matches!(err, CoreError::ChecksumMismatch { .. }), "{err}");
        assert!(err.to_string().contains("expected"), "{err}");

        // Tampering the checksum byte itself also fails.
        let mut bad = frame;
        bad[8] ^= 0x01;
        assert!(parse_response(&cmd, &bad).is_err());
    }

    const NMEA_CMD: &str = r#"
schema: openbaud/command@v0
name: gps_poll
frame: { text: "$GPGGA?\r\n" }
response:
  match: { idle_ms: 20 }
  validate:
    checksum: xor8
    range: { from: 1, to: -4 }
    at: -2
    encoding: ascii_hex
"#;

    /// NMEA-style sentence with a real XOR checksum over the payload.
    fn nmea_frame(payload: &str, uppercase: bool) -> Vec<u8> {
        let x = payload.bytes().fold(0u8, |a, b| a ^ b);
        let tail = if uppercase { format!("{x:02X}") } else { format!("{x:02x}") };
        format!("${payload}*{tail}").into_bytes()
    }

    #[test]
    fn validate_ascii_hex_checksum() {
        let cmd = parse_command(NMEA_CMD, "c.yaml").unwrap();
        assert!(parse_response(&cmd, &nmea_frame("GPGGA,x", true)).is_ok());
        // Comparison is case-insensitive.
        assert!(parse_response(&cmd, &nmea_frame("GPGGA,x", false)).is_ok());

        // A wrong stored value is a checksum mismatch with both values shown.
        let mut bad = nmea_frame("GPGGA,x", true);
        let n = bad.len();
        bad[n - 1] = b'0';
        bad[n - 2] = b'0';
        let err = parse_response(&cmd, &bad).unwrap_err();
        assert!(matches!(err, CoreError::ChecksumMismatch { .. }), "{err}");
        assert!(err.to_string().contains("\"00\""), "{err}");

        // Non-hex characters in the checksum slot are loud.
        let mut junk = nmea_frame("GPGGA,x", true);
        let n = junk.len();
        junk[n - 1] = b'z';
        let err = parse_response(&cmd, &junk).unwrap_err();
        assert!(err.to_string().contains("not a hex byte"), "{err}");
    }

    #[test]
    fn validate_rejects_bad_indices_at_runtime() {
        // Static check passes (mixed sign), runtime inversion is loud.
        let cmd_yaml = SDS011_CMD.replace("{ from: 2, to: 7 }", "{ from: 8, to: -8 }");
        let cmd = parse_command(&cmd_yaml, "c.yaml").unwrap();
        let err = parse_response(&cmd, &sds011_frame([0; 6])).unwrap_err();
        assert!(err.to_string().contains("inverted"), "{err}");

        // Out-of-frame index is loud too.
        let cmd_yaml = SDS011_CMD.replace("at: -2", "at: 40");
        let cmd = parse_command(&cmd_yaml, "c.yaml").unwrap();
        let err = parse_response(&cmd, &sds011_frame([0; 6])).unwrap_err();
        assert!(err.to_string().contains("outside"), "{err}");
    }

    // -- arrays --

    const SCALAR_ARRAY_CMD: &str = r#"
schema: openbaud/command@v0
name: read_samples
frame: { hex: "01" }
response:
  match: { idle_ms: 20 }
  parse:
    fields:
      samples: { at: 1, type: u8, count: 4, scale: 0.5, unit: V }
"#;

    #[test]
    fn scalar_array_fixed_count_with_scale() {
        let cmd = parse_command(SCALAR_ARRAY_CMD, "c.yaml").unwrap();
        let parsed = parse_response(&cmd, &[0xFF, 1, 2, 3, 4]).unwrap();
        assert_eq!(parsed, json!({"samples": [0.5, 1.0, 1.5, 2.0]}));
        assert_eq!(units(&cmd).get("samples").unwrap(), "V");
    }

    const RECORD_ARRAY_CMD: &str = r#"
schema: openbaud/command@v0
name: lidar_scan
frame: { hex: "A5 20" }
response:
  match: { length: 16 }
  parse:
    fields:
      points:
        at: 1
        count: 3
        stride: 5
        elements:
          quality: { at: 0, type: u8 }
          angle:   { at: 1, type: u16le, scale: 0.5 }
          dist_mm: { at: 3, type: u16le }
"#;

    #[test]
    fn record_array_stride_and_element_offsets() {
        let cmd = parse_command(RECORD_ARRAY_CMD, "c.yaml").unwrap();
        // 1 header byte + 3 records x 5 bytes (RPLIDAR-style).
        let mut frame = vec![0xA5];
        for i in 0u16..3 {
            frame.push(10 + i as u8); // quality
            frame.extend((i * 2).to_le_bytes()); // angle raw
            frame.extend((100 + i).to_le_bytes()); // dist_mm
        }
        let parsed = parse_response(&cmd, &frame).unwrap();
        assert_eq!(
            parsed,
            json!({"points": [
                {"quality": 10, "angle": 0.0, "dist_mm": 100},
                {"quality": 11, "angle": 1.0, "dist_mm": 101},
                {"quality": 12, "angle": 2.0, "dist_mm": 102},
            ]})
        );
    }

    const COUNTED_ARRAY_CMD: &str = r#"
schema: openbaud/command@v0
name: read_block
frame: { text: "CURV?\n" }
response:
  match: { idle_ms: 20 }
  parse:
    fields:
      n_points: { at: 0, type: ascii_int, len: 4 }
      points: { at: 4, type: u8, count: { field: n_points } }
"#;

    #[test]
    fn field_driven_count_from_ascii_int_header() {
        let cmd = parse_command(COUNTED_ARRAY_CMD, "c.yaml").unwrap();
        let parsed = parse_response(&cmd, b"   3\x07\x08\x09").unwrap();
        assert_eq!(parsed, json!({"n_points": 3, "points": [7, 8, 9]}));

        // Header says 0: empty array is a legitimate device answer.
        let parsed = parse_response(&cmd, b"   0").unwrap();
        assert_eq!(parsed, json!({"n_points": 0, "points": []}));

        // Non-numeric header is loud.
        let err = parse_response(&cmd, b"abcd\x01").unwrap_err();
        assert!(err.to_string().contains("not a decimal integer"), "{err}");
    }

    #[test]
    fn array_out_of_bounds_is_loud() {
        let cmd = parse_command(SCALAR_ARRAY_CMD, "c.yaml").unwrap();
        let err = parse_response(&cmd, &[0xFF, 1, 2]).unwrap_err();
        assert!(err.to_string().contains("exceeds response length"), "{err}");

        // Field-driven count exceeding the frame is equally loud.
        let cmd = parse_command(COUNTED_ARRAY_CMD, "c.yaml").unwrap();
        let err = parse_response(&cmd, b"  99\x01\x02").unwrap_err();
        assert!(err.to_string().contains("exceeds response length"), "{err}");
    }

    // -- text side: split and hex_int --

    const SPLIT_CMD: &str = r#"
schema: openbaud/command@v0
name: read_trace
frame: { text: "TRACE?\n" }
response:
  match: { delimiter: "\n" }
  parse:
    regex: 'F=(?P<flags>[0-9A-Fa-fx]+) V=(?P<values>.*)$'
    types: { flags: hex_int }
    split:
      values: { sep: ",", type: float }
"#;

    #[test]
    fn split_float_array_and_hex_int_capture() {
        let cmd = parse_command(SPLIT_CMD, "c.yaml").unwrap();
        let parsed = parse_response(&cmd, b"F=0x1AF8 V=1.5, 2.5,3.0,").unwrap();
        // hex_int takes 0x-prefixed or bare hex; split skips empty segments
        // and trims whitespace around each element.
        assert_eq!(parsed, json!({"flags": 0x1AF8, "values": [1.5, 2.5, 3.0]}));

        // Bare hex, case-insensitive.
        let parsed = parse_response(&cmd, b"F=1af8 V=1.0").unwrap();
        assert_eq!(parsed["flags"], json!(0x1AF8));

        // A bad element is loud and names its position.
        let err = parse_response(&cmd, b"F=1 V=1.5,abc,3.0").unwrap_err();
        assert!(err.to_string().contains("element 2"), "{err}");
        assert!(err.to_string().contains("abc"), "{err}");
    }

    #[test]
    fn hex_int_rejects_empty_and_junk() {
        assert_eq!(parse_hex_int("0x1A").unwrap(), 26);
        assert_eq!(parse_hex_int("1A").unwrap(), 26);
        assert_eq!(parse_hex_int("ff").unwrap(), 255);
        assert!(parse_hex_int("").is_err());
        assert!(parse_hex_int("0x").is_err());
        assert!(parse_hex_int("0xZZ").is_err());
        assert!(parse_hex_int("12.5").is_err());
    }

    #[test]
    fn outcome_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Outcome::ChecksumError).unwrap(), "\"checksum_error\"");
        assert_eq!(serde_json::to_string(&Outcome::Normal).unwrap(), "\"normal\"");
        assert_eq!(
            serde_json::from_str::<Outcome>("\"exception\"").unwrap(),
            Outcome::Exception
        );
    }
}
