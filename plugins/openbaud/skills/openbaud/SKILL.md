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
- `capture_start`, `capture_stop`: preserve timing and wire evidence.
- `schema`: obtain the exact knowledge-format contract.

Prefer verified commands over repeated raw byte construction.

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
