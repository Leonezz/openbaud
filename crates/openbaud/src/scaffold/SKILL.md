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

- `devices/<name>/profile.yaml` — transport parameters + default framing + optional selector
- `devices/<name>/commands/*.yaml` — named, typed, executable commands
- `devices/<name>/workflows/*.yaml` — fixed command sequences with a `finally` block
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

Binary field types (hex frames and response fields):
`u8 i8 u16be u16le i16be i16le u32be u32le i32be i32le f32be f32le u32me i32me f32me`.
The `*me` types are CDAB word-swapped ("mid-endian"): low 16-bit word first on
the wire, big-endian within each word — common in Modbus meters (e.g. PZEM-004T).
Text-only param types (for `text:` frame interpolation): `int float string`.
Checksums: `crc16_modbus xor8 sum8` (computed over all preceding frame bytes).

## Outcomes, expectations and protocol exceptions

Every `run_command` execution is classified into one of six outcomes:
`normal` (frame complete, checksum ok, parsed), `exception` (frame recognized
by the `exception` rule), `silence` (zero bytes), `timeout` (partial bytes,
match rule unmet), `checksum_error`, `malformed` (parse failed). The result
JSON always carries `outcome`, `expect`, `expect_met`, `tx_hex`, plus
`raw_hex`/`parsed`/`exception`/`partial_hex`/`detail` as applicable.

A command *declares* which outcome means success with `expect:`
(`normal` — the default — | `exception` | `silence`). An unmet expectation is
an error whose text embeds the full result JSON. This turns negative tests
into first-class commands — a Modbus illegal-function probe:

```yaml
response:
  match: { length: 7 }        # the normal-frame rule (never arrives here)
  expect: exception
  first_byte_ms: 500          # optional: no first byte in 500 ms -> silence early
  exception:                  # how to recognize + decode the exception frame
    when: { at: 1, mask: "FF", equals: "84" }   # (byte[1] & FF) == 84 (FC | 0x80)
    match: { length: 5 }
    validate: { checksum: crc16_modbus }
    parse:
      fields:
        function: { at: 1, type: u8 }
        exception_code: { at: 2, type: u8 }
```

A wrong-station-number probe expects nothing back: `expect: silence` (the
`match` rule may then be omitted; set a short `timeout_ms`).

## Timing

Commands may declare settling delays; the values actually applied are echoed
in the result and audit:

```yaml
timing:
  pre_delay_ms: 100   # wait before sending
  post_delay_ms: 50   # quiet period after the command completes
```

## Workflows (openbaud/workflow@v0)

A workflow is a *fixed sequence* of this device's commands plus a `finally`
block, in `devices/<name>/workflows/*.yaml`:

```yaml
schema: openbaud/workflow@v0
name: pzem_full_check
description: enter Modbus mode, read + exception test, always restore ECHO
steps:
  - command: set_mode_modbus
  - command: read_voltage
    params: { addr: 1 }       # step params override command defaults
  - command: illegal_function_test
finally:
  - command: escape_binary
  - command: set_mode_echo
```

Semantics: steps run in order on one session; the first failing step skips
the rest; `finally` steps are **all attempted regardless**, failures recorded
per step. Workflow risk is the maximum of its commands' risk (`danger` needs
`acknowledge_risk`). Workflow names must not collide with command names.
Execute with the `run_workflow` tool or `openbaud run <device>/<workflow>`.

**Red line: a workflow is not a programming language.** There are no
conditionals, loops, variables or retries — only `steps` and `finally`. If a
sequence needs logic, drive individual commands yourself instead.

## Capture replay

Any place that takes a port also accepts `replay:<path-to-.obcap>` (relative
paths resolve against the workspace). Replay verifies every TX byte-for-byte
against the capture and plays back the recorded RX — a hardware-free
regression check that a command still parses a real device's traffic.

## Device selectors

Give the device a stable identity in `profile.yaml` and the port argument
becomes optional in `open`/`run_command`/`run_workflow`/CLI:

```yaml
selector: { vid: "1A86", pid: "55D3" }   # also: serial_number (exact), product (substring)
```

All present fields must match; exactly one live port may match — zero or
several is a loud error listing the candidates, never a silent pick.

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

`openbaud run <device>/<name>` executes a sedimented command or workflow
without any agent involved (`--port` is optional when the profile has a
selector; `replay:` ports work here too). A device is "done" when its key
commands pass this standalone check — that is the definition of knowledge
worth sharing.
