//! Frame templates.
//!
//! Hex templates are whitespace-separated tokens: even-length hex literals,
//! `{param}` references encoded by the parameter's declared type, and checksum
//! placeholders (any [`ChecksumKind`] name in braces, e.g. `{crc16_modbus}`)
//! computed over every byte built before them.
//!
//! Text templates interpolate `{param}` as strings; `{{` and `}}` escape
//! literal braces.

use crate::checksum::ChecksumKind;
use crate::codec::FieldType;
use crate::{hex, CoreError};
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Bytes(Vec<u8>),
    Param(String),
    Checksum(ChecksumKind),
}

#[derive(Debug, Clone)]
pub struct HexTemplate {
    tokens: Vec<Token>,
}

impl HexTemplate {
    pub fn parse(input: &str) -> crate::Result<Self> {
        let mut tokens = Vec::new();
        for raw in input.split_whitespace() {
            if let Some(name) = raw.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                if name.is_empty() {
                    return Err(CoreError::Template("empty placeholder {}".to_string()));
                }
                match ChecksumKind::from_name(name) {
                    Ok(kind) => tokens.push(Token::Checksum(kind)),
                    Err(_) => tokens.push(Token::Param(name.to_string())),
                }
            } else {
                tokens.push(Token::Bytes(hex::parse_hex(raw)?));
            }
        }
        if tokens.is_empty() {
            return Err(CoreError::Template("template is empty".to_string()));
        }
        Ok(Self { tokens })
    }

    /// Names of all `{param}` placeholders, in order of appearance.
    pub fn param_names(&self) -> Vec<&str> {
        self.tokens
            .iter()
            .filter_map(|t| match t {
                Token::Param(name) => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn build(
        &self,
        values: &Map<String, Value>,
        types: &HashMap<String, FieldType>,
    ) -> crate::Result<Vec<u8>> {
        let mut out = Vec::new();
        for token in &self.tokens {
            match token {
                Token::Bytes(bytes) => out.extend_from_slice(bytes),
                Token::Param(name) => {
                    let value = values.get(name).ok_or_else(|| CoreError::Param {
                        name: name.clone(),
                        reason: "no value provided and no default declared".to_string(),
                    })?;
                    let ty = types.get(name).ok_or_else(|| CoreError::Param {
                        name: name.clone(),
                        reason: "placeholder has no matching entry in params".to_string(),
                    })?;
                    out.extend(ty.encode(name, value)?);
                }
                Token::Checksum(kind) => {
                    let sum = kind.compute(&out);
                    out.extend(sum);
                }
            }
        }
        Ok(out)
    }
}

/// Interpolate `{param}` placeholders in a text template.
pub fn render_text(template: &str, values: &Map<String, Value>) -> crate::Result<String> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => name.push(ch),
                        None => {
                            return Err(CoreError::Template(format!(
                                "unclosed placeholder {{{name}"
                            )))
                        }
                    }
                }
                let value = values.get(&name).ok_or_else(|| CoreError::Param {
                    name: name.clone(),
                    reason: "no value provided and no default declared".to_string(),
                })?;
                match value {
                    Value::String(s) => {
                        // A control character in a *value* would fabricate
                        // wire framing the template never declared (e.g. a
                        // "\n" smuggling a second command line) — rejected
                        // loudly, naming the parameter and the character.
                        // Control characters in the template itself remain
                        // the author's business.
                        if let Some(c) = s.chars().find(|c| c.is_ascii_control()) {
                            return Err(CoreError::Param {
                                name,
                                reason: format!(
                                    "value contains the control character {:?} (U+{:04X}) — \
                                     control characters cannot be interpolated into a text frame; \
                                     declare framing bytes in the template instead",
                                    c, c as u32
                                ),
                            });
                        }
                        out.push_str(s);
                    }
                    Value::Number(n) => out.push_str(&n.to_string()),
                    Value::Bool(b) => out.push_str(if *b { "1" } else { "0" }),
                    other => {
                        return Err(CoreError::Param {
                            name,
                            reason: format!("cannot interpolate {other} into a text frame"),
                        })
                    }
                }
            }
            '}' => {
                return Err(CoreError::Template(
                    "stray '}' — use '}}' for a literal brace".to_string(),
                ))
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn types(pairs: &[(&str, FieldType)]) -> HashMap<String, FieldType> {
        pairs.iter().map(|(n, t)| (n.to_string(), *t)).collect()
    }

    fn values(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs.iter().map(|(n, v)| (n.to_string(), v.clone())).collect()
    }

    #[test]
    fn builds_modbus_read_frame() {
        let t = HexTemplate::parse("{addr} 04 00 00 00 01 {crc16_modbus}").unwrap();
        let frame = t
            .build(&values(&[("addr", json!(1))]), &types(&[("addr", FieldType::U8)]))
            .unwrap();
        assert_eq!(frame.len(), 8);
        assert_eq!(&frame[..6], &[0x01, 0x04, 0x00, 0x00, 0x00, 0x01]);
        // CRC over the first six bytes, little-endian tail.
        let crc = crate::checksum::crc16_modbus(&frame[..6]).to_le_bytes();
        assert_eq!(&frame[6..], &crc);
    }

    #[test]
    fn missing_param_is_loud() {
        let t = HexTemplate::parse("{addr} 04").unwrap();
        let err = t.build(&values(&[]), &types(&[("addr", FieldType::U8)])).unwrap_err();
        assert!(err.to_string().contains("addr"));
    }

    #[test]
    fn param_names_extraction() {
        let t = HexTemplate::parse("{a} 01 {b} {sum8}").unwrap();
        assert_eq!(t.param_names(), vec!["a", "b"]);
    }

    #[test]
    fn control_characters_in_string_params_are_rejected_loudly() {
        // A newline in a parameter value would fabricate extra protocol lines
        // ("MODE=ECHO\nINJECTED=1" goes on the wire as two commands).
        for bad in ["ECHO\nINJECTED=1", "a\rb", "nul\0", "del\x7fchar", "tab\tsplit"] {
            let err = render_text("MODE={mode}\r\n", &values(&[("mode", json!(bad))]))
                .expect_err("control characters must be rejected");
            let msg = err.to_string();
            assert!(msg.contains("mode"), "error must name the parameter, got: {msg}");
            assert!(msg.contains("control character"), "got: {msg}");
        }
        // The error names the offending character.
        let err = render_text("M={m}", &values(&[("m", json!("x\ny"))])).unwrap_err();
        assert!(err.to_string().contains("\\n"), "got: {err}");

        // Normal strings pass, and a literal backslash-n (two characters) is
        // data, not a control character.
        assert_eq!(
            render_text("M={m}", &values(&[("m", json!("plain ok"))])).unwrap(),
            "M=plain ok"
        );
        assert_eq!(
            render_text("M={m}", &values(&[("m", json!("back\\nslash"))])).unwrap(),
            "M=back\\nslash"
        );
        // Control characters in the template itself stay allowed — they are
        // the author's declared framing, not injected data.
        assert_eq!(
            render_text("AT+X={m}\r\n", &values(&[("m", json!("ok"))])).unwrap(),
            "AT+X=ok\r\n"
        );
    }

    #[test]
    fn text_interpolation_and_escapes() {
        let out = render_text(
            "AT+CWJAP={ssid},{{literal}}\r\n",
            &values(&[("ssid", json!("lab"))]),
        )
        .unwrap();
        assert_eq!(out, "AT+CWJAP=lab,{literal}\r\n");
        assert!(render_text("{oops", &values(&[])).is_err());
    }
}
