//! Command execution semantics: parameter resolution, frame building and
//! response parsing. IO-free — the engine feeds raw bytes in and out.

use crate::codec::FieldType;
use crate::checksum::ChecksumKind;
use crate::format::{Command, Expect, ExceptionSpec, ParseSpec, ValidateSpec};
use crate::template::{render_text, HexTemplate};
use crate::CoreError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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
        ChecksumKind::from_name(&validate.checksum)?.verify_frame(raw)?;
    }
    let Some(parse) = &resp.parse else {
        return Ok(Value::Object(Map::new()));
    };
    parse_with_spec(parse, raw)
}

fn parse_with_spec(parse: &ParseSpec, raw: &[u8]) -> crate::Result<Value> {
    let mut out = Map::new();
    if let Some(fields) = &parse.fields {
        for (name, field) in fields {
            let ty = FieldType::from_name(&field.type_name)?;
            let value = ty.decode(raw, field.at)?;
            let value = match field.scale {
                Some(scale) => {
                    let n = value.as_f64().expect("binary decode always yields a number");
                    Value::from(round10(n * scale))
                }
                None => value,
            };
            out.insert(name.clone(), value);
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
            let coerce = parse
                .types
                .as_ref()
                .and_then(|t| t.get(name))
                .map(String::as_str)
                .unwrap_or("string");
            let value = match coerce {
                "int" => Value::from(m.as_str().parse::<i64>().map_err(|_| {
                    CoreError::Parse(format!("capture {name:?}={:?} is not an int", m.as_str()))
                })?),
                "float" => Value::from(m.as_str().parse::<f64>().map_err(|_| {
                    CoreError::Parse(format!("capture {name:?}={:?} is not a float", m.as_str()))
                })?),
                _ => Value::from(m.as_str()),
            };
            out.insert(name.to_string(), value);
        }
    }
    Ok(Value::Object(out))
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
        let checked = ChecksumKind::from_name(&v.checksum).and_then(|k| k.verify_frame(&bytes));
        if let Err(e) = checked {
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
