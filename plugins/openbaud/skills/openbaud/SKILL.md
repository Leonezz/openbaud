---
name: openbaud
description: Explore, understand, profile, and automate local serial or USB hardware with the bundled OpenBaud MCP tools. Use for serial devices, Modbus, radar sensors, protocol reverse engineering, captures, timing analysis, device commands, and device workflows.
---

# OpenBaud device work

Use OpenBaud as the audited capability layer between the agent and local serial
hardware. Do not replace its structured tools with ad-hoc shell serial access.

## Start safely

1. Call `list_ports` and identify physical devices by USB VID/PID, serial number,
   manufacturer, and product. On macOS, prefer `/dev/cu.*` for initiating a
   connection.
2. Inspect `devices/` for an existing device profile before probing.
3. Read available documentation before sending bytes. If documentation is not
   available, state the hypothesis and start with read-only or mock/replay tests.
4. Start a capture before a longer live exploration so conclusions have wire
   evidence.

## Sediment knowledge

OpenBaud work is complete only when verified behavior becomes reusable project
knowledge:

- `devices/<name>/profile.yaml`: transport, framing, and stable port selector.
- `devices/<name>/commands/*.yaml`: typed, named protocol operations.
- `devices/<name>/workflows/*.yaml`: fixed sequences and cleanup steps.
- `devices/<name>/notes.md`: evidence, quirks, unknowns, and datasheet references.
- `captures/*.obcap`: timestamped RX/TX evidence.

Before writing a profile, command, or workflow, call the `schema` tool for that
kind. Its output is authoritative; do not guess field names from examples.

After manual verification, encode the behavior as a command and rerun it through
`run_command`. Add provenance only when a datasheet page or real capture supports
the claim. Use `replay:<capture path>` for hardware-free regression tests.

## Choose the narrowest tool

- `list_ports`: discover live ports and the `mock:echo` test transport.
- `open`, `read`, `close`: observe an existing stream or session.
- `request`: perform a bounded exploratory request with an explicit match rule.
- `send`: raw writes only when a response is not expected.
- `run_command`: execute verified declarative device behavior.
- `run_workflow`: execute a verified fixed sequence with cleanup.
- `capture_start`, `capture_stop`: preserve timing and wire evidence. Both
  return the capture's workspace-relative `path` (`captures/<file>`) — pass
  it verbatim to `capture_frames`, `session_timeline`, or `replay:<path>`.
- `session_timeline`: fold a recorded capture plus the audit log into a
  density-and-events timeline of that session (capture file required; no
  live-session mode).
- `capture_frames`: re-frame a capture's bytes into timestamped tx/rx frames,
  paginated; pass an explicit `framing` or a device whose profile declares one
  — one of the two is required.
- `diagnose_frame`: probe one frame against every checksum algorithm and
  encoding at the tail (each row's `at` is where the stored checksum starts;
  inapplicable algorithms report only the error), and — with
  `expected: {device, command}` — against that command's parse at byte
  offsets -2..=+2. `parsed: true` only means structurally decodable at that
  offset; the offsets are mutually exclusive hypotheses to judge yourself.
  Pure computation, no hardware.
- `session_stats`: live counters for open sessions (buffered, dropped, rx/tx
  bytes, transport, capture state).
- `stream_poll`: per-consumer incremental frame subscription on a live
  session — create with `session_id`, poll with the returned
  `subscription_id`, acknowledge with `since_seq` set to the last delivered
  frame's `seq + 1` (unacknowledged frames are redelivered, so a lost
  response is recoverable; acking frames never delivered is a loud error).
  Pages stop at `max_frames` and at `max_inline_bytes` (default 4096, bounds
  512..=262144; metered as rendered hex + text length, whole frames only,
  actual total reported as `page_bytes`) — except a single first frame beyond
  the budget, delivered alone with `oversized_frame: true` so delivery always
  makes progress.
  Lagging drops the oldest frames into a loud `dropped_frames` count and
  buffer overflow past the cursor into `dropped_chunks`; a drained poll on a
  dead port errors loudly; `close: true` releases it. Independent of `read`
  — the two never steal frames from each other. Pass `parse: {device,
  command}` when *creating* the subscription (never on a follow-up poll —
  loud error) to parse every frame server-side with that workspace command's
  `response.parse`: frames then carry `parsed` (field values) or a per-frame
  `parse_error` (one bad frame never stops the stream), parsed once at
  arrival so redelivery repeats the identical outcome; results echo
  `parse: {device, command}` plus the command's `units` (`run_command`
  semantics), and an unknown device/command or a command without a
  `response.parse` block fails creation loudly.
- `schema`: obtain the exact knowledge-format contract.

Prefer verified commands over repeated raw byte construction.

Two tools address the user rather than the hardware, so invoke them
deliberately and not by habit:

- `show_result`: render a saved result for the user, given the `path` from an
  earlier `full_result`. The view reads the complete payload itself, so large
  data reaches the user without entering your context. Use it when the shape of
  the data is the point; say a two-field answer instead of drawing it.
- `ask_port`: ask which port to use when several could be the device and the
  choice is the user's. It opens nothing — the answer returns to you and you
  call `open`. On hosts that never deliver the selection back, ask the user
  directly instead of guessing. For your own discovery use `list_ports`,
  which reports device matches, ports held by a session, and `/dev/tty.*`
  twins.

## Declarative rendering

Results are drawn only from declared encodings — never guessed from field
names:

- A command result charts only when its YAML declares `response.view`, naming
  which parsed field feeds which visual channel, e.g.
  `view: { kind: polar, data: points, angle: bearing, radius: range_mm }`.
  Declare it once and every `run_command` result renders through
  `show_result`.
- For bytes you decoded yourself, pass the same channel mapping as the
  `encoding` argument of `show_result`.
- `session_timeline`, `capture_frames` and `diagnose_frame` results carry
  their own `view` declaration (`timeline`, `capture`, `diagnostics`): hand
  the result, or its `full_result` path, to `show_result` and it renders — no
  extra encoding needed.
- No declaration means no chart: the result renders as an honest field table.

## Safety boundary

Serial writes can reconfigure, actuate, or permanently damage hardware.

- Explain the expected effect before any write.
- Treat unknown raw frames as potentially destructive.
- Never set `acknowledge_risk: true` without explicit user confirmation of the
  described consequence.
- Stop transmitting when baud, framing, checksum, or device identity is unclear.
- Keep secrets and sensitive payloads out of notes and shared device packs.

## Analysis and visualization

For radar, heatmap, or timing work, preserve the lossless capture first. Parse
frames into named values with units, timestamps, and provenance, then build the
visualization from those structured results. Keep the capture path alongside the
derived artifact so the chart remains auditable.
