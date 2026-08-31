//! Machine-readable descriptions of the three knowledge formats: JSON Schema
//! generated from the same serde types that parse them (so schema and parser
//! cannot drift), plus annotated YAML examples guaranteed to load.

use crate::format::{Command, Profile, Workflow, COMMAND_SCHEMA, PROFILE_SCHEMA, WORKFLOW_SCHEMA};
use serde_json::Value;

/// Which knowledge format to describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaKind {
    Profile,
    Command,
    Workflow,
}

/// Semantic rules the JSON Schema grammar cannot express, surfaced in the
/// root description so an agent sees them alongside the structure.
const PROFILE_RULES: &str = "Semantic rules beyond this schema: \
framing sets exactly one of delimiter / idle_ms / length_prefix; \
selector, when present, sets at least one of vid / pid / serial_number / product \
(vid/pid are 1-4 hex digits); \
transport.data_bits is 5..=8 and transport.stop_bits is 1 or 2.";

const COMMAND_RULES: &str = "Semantic rules beyond this schema: \
frame sets exactly one of hex / text; \
response.match sets exactly one of length / delimiter / idle_ms and is required unless expect: silence; \
expect: exception requires a response.exception block; \
parse sets exactly one of fields (binary) / regex (text); \
every {param} placeholder in the frame must be declared in params, and hex frames need binary param types; \
a string param interpolated into a text frame must not contain any control character \
(0x00-0x1F, 0x7F) — framing bytes like \\r\\n belong in the template, never in a value; \
hex-frame checksum placeholders ({crc16_modbus}, {crc16_ccitt}, {crc8}, {crc32}, {xor8}, {sum8}, {sum16be}) \
cover every byte built before them, and validate.checksum takes the same names; \
a parse field sets exactly one of type / elements — elements makes a one-level record array and requires count and stride; \
type ascii_int requires len; \
count: {field: name} must reference a scalar field of the same parse block; \
a field decodes as value = ((raw >> bits.lsb) & mask(bits.width)) x scale + offset — \
bits only on unsigned integer types (u8, u16be, u16le, u32be, u32le, u32me) with lsb + width within the type's bit width, \
and setting scale or offset makes the value a float; \
scale/offset/bits/unit go on the element fields of a record array, not on its container; \
types values and split.type are either a bare type name (int, float, string, hex_int) \
or an object {type, bits, scale, offset} running the same pipeline — \
bits only for int/hex_int (a negative value is a runtime error), scale/offset not for string; \
split only applies to regex parsing, its keys must be named captures, and a capture cannot appear in both types and split; \
validate.range and validate.at accept negative indices counted from the frame end (-1 = last byte); \
validate defaults reproduce the classic tail checksum over all preceding bytes; \
response.view declares how a result is drawn by naming which parse field feeds which visual channel — \
its data must be a record-array field of response.parse.fields, and every channel must name one of that array's elements, \
so a device keeps its own field names and nothing is ever inferred from naming; \
kind: polar requires both angle and radius (intensity optional), \
and a channel whose field declares a unit of the wrong kind (an angle channel on a length field, or the reverse) is rejected; \
kind: heatmap requires data to name a scalar array parse field plus rows and cols with rows x cols >= 1 \
(cells fill the grid row-major, units ride on the data field); \
polar and heatmap are the only kinds a command YAML may declare — the timeline / diagnostics / capture kinds are \
produced by tools (session_timeline / diagnose_frame / capture_frames), the scope kind is declared by the \
live-stream descriptor an agent passes to show_result, and all of them are rejected here; \
a command with no view is simply never charted.";

const WORKFLOW_RULES: &str = "Semantic rules beyond this schema: \
steps must not be empty; \
every referenced command must exist in the same device directory; \
finally steps always run, even after a failed step.";

/// The JSON Schema for one knowledge format, with `$id` set to the format
/// identifier ("openbaud/<kind>@v0") and the semantic rules appended to the
/// root description.
pub fn json_schema(kind: SchemaKind) -> Value {
    let (mut value, id, rules) = match kind {
        SchemaKind::Profile => {
            (schemars::schema_for!(Profile).to_value(), PROFILE_SCHEMA, PROFILE_RULES)
        }
        SchemaKind::Command => {
            (schemars::schema_for!(Command).to_value(), COMMAND_SCHEMA, COMMAND_RULES)
        }
        SchemaKind::Workflow => {
            (schemars::schema_for!(Workflow).to_value(), WORKFLOW_SCHEMA, WORKFLOW_RULES)
        }
    };
    let obj = value.as_object_mut().expect("a derived root schema is always a JSON object");
    let description = match obj.get("description").and_then(Value::as_str) {
        Some(d) => format!("{d}\n\n{rules}"),
        None => rules.to_string(),
    };
    obj.insert("$id".to_string(), Value::from(id));
    obj.insert("description".to_string(), Value::from(description));
    value
}

/// An annotated YAML example for one knowledge format. Every document in the
/// returned string loads through the corresponding `parse_*` function (the
/// command example holds two documents separated by `---`: one binary, one
/// text).
pub fn example(kind: SchemaKind) -> &'static str {
    match kind {
        SchemaKind::Profile => PROFILE_EXAMPLE,
        SchemaKind::Command => COMMAND_EXAMPLE,
        SchemaKind::Workflow => WORKFLOW_EXAMPLE,
    }
}

const PROFILE_EXAMPLE: &str = r#"# openbaud profile example - device identity and serial line settings.
schema: openbaud/profile@v0
name: pzem004t
description: PZEM-004T v3 power meter
transport:
  baud: 9600        # default 115200
  data_bits: 8      # 5..=8, default 8
  parity: none      # none | even | odd
  stop_bits: 1      # 1 or 2
framing:            # exactly one of: delimiter, idle_ms, length_prefix
  idle_ms: 30       # frame ends after 30 ms of line silence
selector:           # all present fields must match (AND)
  vid: "1a86"       # 1-4 hex digits, case-insensitive
  pid: "7523"
  product: "CH340"  # substring match
"#;

const COMMAND_EXAMPLE: &str = r#"# openbaud command examples - two documents: binary response, then text.
schema: openbaud/command@v0
name: read_waveform
description: Read a waveform block (validate range, ascii_int, scalar and record arrays)
risk: read          # read | write | danger
params:
  - { name: channel, type: u8, default: 1, min: 1, max: 4, description: source channel }
frame:
  # Whitespace-separated hex bytes; {channel} is encoded per its param type,
  # {sum16be} covers every byte built before it.
  hex: "AA 01 {channel} {sum16be}"
response:
  match: { length: 26 }        # exactly one of: length, delimiter, idle_ms
  timeout_ms: 2000
  validate:
    checksum: sum8             # crc16_modbus | crc16_ccitt | crc8 | crc32 | xor8 | sum8 | sum16be
    range: { from: 2, to: -3 } # bytes covered, inclusive; negative = from frame end
    at: -2                     # checksum position; default is the frame tail
  parse:
    fields:
      # ascii_int: len bytes of ASCII decimal digits (leading spaces/zeros ok).
      n_samples: { at: 2, type: ascii_int, len: 4 }
      # Linear calibration is value = raw x scale + offset (scale defaults 1,
      # offset 0); setting either makes the value a float.
      temp_c: { at: 5, type: u8, offset: -40, unit: C }
      # Scalar array: count may reference a scalar field decoded first.
      samples:
        at: 6
        type: u8
        count: { field: n_samples }
        scale: 0.0392          # applied per element
        unit: V
      # Record array: stride bytes per record; element offsets are record-relative.
      # bits extracts ((raw >> lsb) & mask(width)) before scale/offset; lsb/width
      # map onto register-table [msb:lsb] notation (width = msb - lsb + 1), only
      # on unsigned integer types. Several fields may share one byte offset.
      points:
        at: 6
        count: 3               # or count: { field: ... }
        stride: 5
        elements:
          start_flag: { at: 0, type: u8,    bits: { lsb: 0, width: 1 } }
          quality:    { at: 0, type: u8,    bits: { lsb: 2, width: 6 } }
          angle_deg:  { at: 1, type: u16le, bits: { lsb: 1, width: 15 }, scale: 0.015625, unit: deg }
          dist_mm:    { at: 3, type: u16le, scale: 0.25, unit: mm }
provenance:
  datasheet: "datasheet.pdf#page=12"
---
# Text response: ascii_hex checksum, regex parsing, split arrays and hex_int.
schema: openbaud/command@v0
name: poll_status
description: Poll a text status line (NMEA-style checksum)
frame: { text: "STATUS?\r\n" }
response:
  match: { delimiter: "\r\n" }
  validate:
    checksum: xor8
    range: { from: 1, to: -6 } # after the leading '$' up to before the '*'
    at: -4                     # the two hex characters before "\r\n"
    encoding: ascii_hex        # checksum appears as ASCII hex, case-insensitive
  parse:
    regex: 'S,(?P<flags>[0-9A-Fa-f]+),(?P<temp>[0-9A-Fa-f]+),(?P<values>[^*]*)\*'
    types:
      flags: hex_int           # bare form: int | float | string | hex_int
      # Object form runs the same pipeline as binary fields:
      # value = ((converted >> lsb) & mask(width)) x scale + offset.
      # bits only for int/hex_int (negative values error at runtime).
      temp: { type: hex_int, offset: -40 }
    split:
      # Split the "values" capture on "," and convert each piece (the element
      # type takes the bare or the object form too).
      values: { sep: ",", type: float }
"#;

const WORKFLOW_EXAMPLE: &str = r#"# openbaud workflow example - a fixed command sequence with cleanup.
schema: openbaud/workflow@v0
name: full_check
description: read everything, then restore echo mode
steps:
  - command: set_mode_modbus
  - command: read_voltage
    params: { addr: 1 }        # values for the command's declared params
finally:                       # always runs, even if a step fails
  - command: set_mode_echo
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{parse_command, parse_profile, parse_workflow};

    #[test]
    fn schemas_carry_id_and_rules() {
        for (kind, id) in [
            (SchemaKind::Profile, "openbaud/profile@v0"),
            (SchemaKind::Command, "openbaud/command@v0"),
            (SchemaKind::Workflow, "openbaud/workflow@v0"),
        ] {
            let schema = json_schema(kind);
            assert_eq!(schema["$id"], id);
            let desc = schema["description"].as_str().unwrap();
            assert!(desc.contains("Semantic rules"), "{kind:?}: {desc}");
            assert_eq!(schema["type"], "object");
            assert!(schema["properties"].is_object());
            // deny_unknown_fields must surface as additionalProperties: false.
            assert_eq!(schema["additionalProperties"], false, "{kind:?}");
        }
    }

    #[test]
    fn command_schema_reflects_serde_attributes() {
        let schema = json_schema(SchemaKind::Command);
        let text = serde_json::to_string(&schema).unwrap();
        // rename: FieldSpec's type_name appears as "type", match_spec as "match".
        let defs = &schema["$defs"];
        assert!(defs["FieldSpec"]["properties"]["type"].is_object());
        assert!(defs["ResponseSpec"]["properties"]["match"].is_object());
        // rename_all lowercase enums.
        assert!(text.contains("\"danger\""));
        assert!(text.contains("\"silence\""));
        // New validate/array/split/bits vocabulary is present.
        for key in [
            "range", "encoding", "ascii_hex", "count", "stride", "elements", "split", "bits",
            "offset", "lsb", "width", "heatmap", "scope", "rows", "cols",
        ] {
            assert!(text.contains(key), "schema misses {key:?}");
        }
        // untagged CountSpec and TextType become unions, not externally-tagged
        // objects.
        for def in ["CountSpec", "TextType"] {
            let union = serde_json::to_string(&defs[def]).unwrap();
            assert!(union.contains("anyOf") || union.contains("oneOf"), "{def}: {union}");
            assert!(!union.contains("\"Fixed\"") && !union.contains("\"Name\""), "{def}: {union}");
        }
    }

    #[test]
    fn command_rules_list_all_checksums_and_tool_produced_view_kinds() {
        let schema = json_schema(SchemaKind::Command);
        let desc = schema["description"].as_str().unwrap();
        for name in ["crc16_modbus", "crc16_ccitt", "crc8", "crc32", "xor8", "sum8", "sum16be"] {
            assert!(desc.contains(name), "rules miss checksum {name:?}");
        }
        for tool in ["session_timeline", "diagnose_frame", "capture_frames"] {
            assert!(desc.contains(tool), "rules miss producing tool {tool:?}");
        }
        for needle in ["heatmap", "rows", "cols", "scope", "stream"] {
            assert!(desc.contains(needle), "rules miss view vocabulary {needle:?}");
        }
        assert!(
            desc.contains("polar and heatmap"),
            "rules must name polar and heatmap as the declarable kinds: {desc}"
        );
        assert!(
            desc.contains("control character"),
            "rules must state that text-frame string params reject control characters: {desc}"
        );
    }

    #[test]
    fn examples_parse_with_their_own_parsers() {
        parse_profile(example(SchemaKind::Profile), "example.yaml").unwrap();
        parse_workflow(example(SchemaKind::Workflow), "example.yaml").unwrap();
        let docs: Vec<&str> = example(SchemaKind::Command).split("\n---\n").collect();
        assert_eq!(docs.len(), 2, "command example should hold two documents");
        for doc in docs {
            parse_command(doc, "example.yaml").unwrap();
        }
    }
}
