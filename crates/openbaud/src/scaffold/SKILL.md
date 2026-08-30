---
name: openbaud
description: Explore, understand, and automate serial or USB hardware through the OpenBaud MCP tools. Use when connecting to a serial device, reverse-engineering or implementing a protocol from a datasheet, or turning verified behavior into reusable commands.
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
- `.openbaud/out/` — full copies of oversized tool results (see `full_result` below)

## Exploration workflow

1. **Identify**: `list_ports` — USB VID/PID and product strings often identify
   the adapter or device. Check `devices/` first: knowledge may already exist.
2. **Read the datasheet before sending anything.** Extract candidate commands,
   frame structure, checksum algorithm, baud rate. Record page references.
3. **Hypothesize → verify in small steps**: `open` the port, use `request`
   with an explicit `match` rule. Start a capture (`capture_start`) before
   longer experiments so evidence is preserved. `capture_start`/`capture_stop`
   return the capture's workspace-relative `path` (`captures/<file>`) — pass
   it verbatim to `capture_frames`, `session_timeline`, or `replay:<path>`.
4. **Record as you go** in `devices/<name>/notes.md`: what you sent, what came
   back, what it means, what surprised you.
5. **Sediment**: once a behavior is confirmed, write it as a command YAML (see
   below). Then re-verify through `run_command` — the declarative spec must
   reproduce the hand-built result.
6. **Mark provenance**: add `provenance.datasheet` (page reference) and, after
   verifying against the live device, `provenance.verified` with the capture
   path and what you observed. Never mark `verified` without a real device
   interaction backing it.

## The YAML formats: `schema` is the single authority

The complete, always-current reference for the three formats (profile,
command, workflow) is the **`schema` MCP tool** — `kind:
profile|command|workflow`, plus `example: true` for an annotated YAML sample —
or, outside MCP, `openbaud schema <kind> [--example]`. It is generated from
the exact code that parses the files and includes the semantic rules the
schema grammar cannot express. **Call it before writing or editing any YAML
under `devices/`** instead of guessing field names or type lists.

The vocabulary covers, beyond the basics: binary/scalar and record **arrays**
(`count`/`stride`/`elements`), text **`split`** arrays, `hex_int` captures,
and checksum `validate` with `range`/`at`/`encoding: ascii_hex`.

A short taste of a command (`openbaud/command@v0`):

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
provenance:
  datasheet: "datasheet.pdf#page=7"
  verified: { capture: "captures/cap-....obcap", note: "matches multimeter", date: 2026-08-22 }
```

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
into first-class commands: a Modbus illegal-function probe declares
`expect: exception` plus an `exception` recognizer block; a wrong-station
probe declares `expect: silence` (the `match` rule may then be omitted; set a
short `timeout_ms`). See the `schema` tool for the exact syntax, including
`timing` settling delays.

## Oversized results

A result JSON larger than `max_inline_bytes` (default 4096; a tool parameter
and a `--max-inline-bytes` CLI flag) is written in full to `.openbaud/out/`
and returned as a summary: long strings and arrays are visibly truncated and
the top-level `full_result` field points at the complete file — read it from
there when you need every element.

## Showing the user, and asking the user

Two tools exist only to put something in front of the person. Every other tool
is yours to call freely and silently; these two are deliberate acts, so use
them when the user is the audience — not as a habit.

- `show_result` renders a saved result on hosts that can draw it. A result is
  charted only when it says how: the command's `response.view` names which
  parsed field feeds which visual channel (`{kind: polar, data: points, angle:
  bearing, radius: range_mm}`), so a device keeps its own field names and
  openbaud never guesses a chart from naming. Declare it once in the command
  YAML and every result renders; pass `encoding` to `show_result` for the
  ad-hoc case where you decoded the bytes yourself. No declaration, no chart —
  the result renders as a field table. Pass the `path` from an earlier
  `full_result`: the view reads the complete payload itself, so a 36-point scan
  reaches the user's eyes without any of it entering your context. Reach for it
  when the shape of the data is the point — scans, waveforms, long field sets —
  and not for a two-field answer you can simply say.
- `ask_port` asks which port to use when several could plausibly be the device
  and the choice is genuinely the user's. It touches no hardware: the answer
  arrives in your context and **you** then call `open`. For your own discovery
  use `list_ports`, which already reports which workspace devices each port
  matches, which ports a session is holding, and which are `/dev/tty.*` twins
  of a canonical `/dev/cu.*` entry.

On hosts without rendering both degrade to plain text — a candidate list you
relay yourself, a pointer you can read from disk. Nothing breaks; the user just
reads instead of looks.

## Reading back what happened: the data tools

Five read-only tools analyze what is already on disk or in memory. They never
touch the wire and are yours to call freely:

- `session_timeline` reads a capture (`path: captures/….obcap`) plus the audit
  log and lays the capture's session out as a timeline: rx/tx byte density
  folded into buckets, and the audited operations (commands, raw writes,
  workflow steps, denials) as events. Window with `from_ms`/`to_ms`. It needs
  a capture file — there is no live-session mode.
- `capture_frames` re-frames a capture's byte stream — an explicit `framing`
  object or a `device` whose profile declares one; one of the two is required,
  nothing is assumed — into timestamped tx/rx frames, paginated with
  `cursor`/`max_frames`. Idle-gap framing is driven by the recorded
  timestamps, so the boundaries match what a live session would have produced.
- `diagnose_frame` takes one frame as `hex` and verifies every checksum
  algorithm x encoding at the tail (`checksum_matrix`; each row's `at` is the
  byte offset where the stored checksum starts, and a row whose algorithm
  cannot fit the frame carries only the error, no position); with
  `expected: {device, command}` it also attempts that command's declared parse
  at byte offsets -2..=+2 (`parse_attempts`) to expose off-by-N framing.
  `parsed: true` only means the bytes decode structurally at that offset —
  the offsets are mutually exclusive hypotheses, so judge which one is the
  real alignment yourself. Pure computation.
- `session_stats` reports live counters for open sessions: buffered/dropped
  bytes, rx/tx totals, transport settings, and the active capture.
- `stream_poll` gives you a private incremental cursor over a live session's
  frames, fully independent of `read` — the two never steal frames from each
  other. First call with `session_id` creates a subscription and returns its
  `subscription_id`; later calls with that id return the frames received
  since, each carrying a monotonic `seq`. Pass `since_seq` to acknowledge
  delivery (frames with `seq < since_seq` are released) — ack with the last
  delivered frame's `seq + 1`, not `next_seq`, which can run ahead of a
  capped page; acking frames never delivered is a loud error. Each page is
  capped by `max_frames` and by `max_inline_bytes` (default 4096, bounds
  512..=262144), a budget metered as each frame's rendered hex length + text
  length — pages hold whole frames only, in `seq` order, and `page_bytes`
  reports the page's actual total. A first frame alone beyond the budget is
  still delivered — alone, with `oversized_frame: true` — so delivery always
  makes progress even through idle-merged megabyte frames.
  Omit `since_seq` and the unacknowledged frames come back unchanged, so a
  lost response costs nothing. A consumer that lags loses the oldest retained
  frames, counted in `dropped_frames`; session-buffer overflow past the
  subscription's cursor is counted in `dropped_chunks` — never silently. A
  poll with nothing left to deliver on a dead port errors loudly instead of
  looking healthy. `close: true` releases the subscription; ones idle beyond
  120 s are swept by a later `stream_poll` call against the session (there is
  no background sweeper), at most 8 exist per session, and every result folds
  in the session's live counters as `stats`. It never waits — poll again when
  you expect more.

`session_timeline`, `capture_frames` and `diagnose_frame` results carry their
own `view` declaration (`timeline`, `capture`, `diagnostics`) — hand the
result, or its `full_result` path, to `show_result` and it renders; no extra
`encoding` needed.

## Workflows (openbaud/workflow@v0)

A workflow is a *fixed sequence* of this device's commands plus a `finally`
block, in `devices/<name>/workflows/*.yaml` (steps may carry `params`
overriding command defaults — `schema` tool has the format).

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
