---
name: openbaud
description: Explore, understand and automate serial/hardware devices through the openbaud MCP tools. Use when connecting to a serial device, reverse-engineering or implementing a device protocol from a datasheet, or sedimenting device knowledge into reusable commands.
---

# Working with serial devices through openbaud

openbaud gives you audited serial-port access (MCP tools) plus a knowledge
format in this workspace. Your goal when touching a device is never just to
make one interaction work — it is to **sediment verified knowledge** so the
next session (or the next person) starts where you finished.

## Workspace layout

- `devices/<name>/profile.yaml` — transport parameters + default framing
- `devices/<name>/commands/*.yaml` — named, typed, executable commands
- `devices/<name>/notes.md` — quirks, datasheet errata, open questions
- `captures/*.obcap` — lossless wire captures (JSONL: ts_ms, dir, hex)
- `.openbaud/audit.jsonl` — every write you performed, append-only

## Exploration workflow

1. **Identify**: `list_ports` — USB VID/PID and product strings often identify
   the adapter or device. Check `devices/` first: knowledge may already exist.
2. **Read the datasheet before sending anything.** Extract candidate commands,
   frame structure, checksum algorithm, baud rate. Record page references.
3. **Hypothesize → verify in small steps**: `open` the port, use `request`
   with an explicit `match` rule. Start a capture (`capture_start`) before
   longer experiments so evidence is preserved.
4. **Record as you go** in `devices/<name>/notes.md`: what you sent, what came
   back, what it means, what surprised you.
5. **Sediment**: once a behavior is confirmed, write it as a command YAML (see
   below). Then re-verify through `run_command` — the declarative spec must
   reproduce the hand-built result.
6. **Mark provenance**: add `provenance.datasheet` (page reference) and, after
   verifying against the live device, `provenance.verified` with the capture
   path and what you observed. Never mark `verified` without a real device
   interaction backing it.

## Command format (openbaud/command@v0)

```yaml
schema: openbaud/command@v0
name: read_voltage
risk: read                  # read | write | danger
params:
  - { name: addr, type: u8, default: 1, min: 1, max: 247 }
frame:
  hex: "{addr} 04 00 00 00 01 {crc16_modbus}"   # or text: "AT+CSQ\r\n"
response:
  match: { length: 7 }      # or delimiter / idle_ms
  validate: { checksum: crc16_modbus }
  parse:
    fields:
      voltage: { at: 3, type: u16be, scale: 0.1, unit: "V" }
    # text protocols instead: regex with named captures + types: {rssi: int}
provenance:
  datasheet: "datasheet.pdf#page=7"
  verified: { capture: "captures/cap-....obcap", note: "matches multimeter", date: 2026-08-22 }
```

Field types: `u8 i8 u16be u16le i16be i16le u32be u32le i32be i32le f32be f32le`;
checksums: `crc16_modbus xor8 sum8` (computed over all preceding frame bytes).

## Safety rules

- Writes to hardware are irreversible. Before any `send`/`request` that
  changes device state, say what you expect it to do.
- Classify honestly: `read` never mutates; `write` mutates recoverably;
  `danger` can brick, misconfigure persistently, or actuate something
  physical. `danger` commands require `acknowledge_risk: true` — confirm with
  the user first, and document the consequence in the command description.
- If output looks wrong (framing garbage, wrong baud), stop sending and fix
  the transport parameters instead of retrying blindly.
- `mock:echo` is always available to smoke-test framing and tooling without
  hardware.

## Verification discipline

`openbaud run <device>/<command> --port <p>` executes a sedimented command
without any agent involved. A device is "done" when its key commands pass this
standalone check — that is the definition of knowledge worth sharing.
