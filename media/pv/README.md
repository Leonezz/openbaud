# OpenBaud product video

This directory is the reproducible Remotion source for the 46-second OpenBaud
PV. The story is agent-first: a user states a hardware goal in natural
language, the agent declares its safety strategy, calls OpenBaud tools, turns
verified behavior into a reviewable command, and reuses that capability in a
future task without the board. The radar animation is supporting evidence,
generated from
`public/obp1-radar-8-scans.obcap`, a real OpenBaud capture recorded from the
ESP32-S3 test board. `pnpm extract` validates each CRC and materializes the eight
36-point frames in `src/radar-scans.json`.

The agent sequences use a purpose-built Codex screen reconstruction based on a
current macOS Codex window reference. It reproduces the real app's workspace
layout, transcript density, inline tool states, output blocks, edit summary,
composer, cursor movement, and task progress. No private task screenshot is
embedded in the public video; all on-screen task names and transcript content
are staged for this demonstration.

## Build

```sh
pnpm install
pnpm extract
pnpm studio
pnpm still
pnpm render
```

`pnpm render` produces the silent H.264 master under the repository's ignored
`dist/` directory. `pnpm final` generates the programmatic ambient track with
FFmpeg, muxes the public master, and builds the GitHub preview GIF.

## Provenance

- Real evidence: `public/obp1-radar-8-scans.obcap`
- Test-data disclosure: OBP/1 response flag bit 0 is `simulated_scene`
- AIGC asset: `public/esp32-workbench-aigc.png`
- AIGC role: atmosphere plate in the opening scene only, never device evidence
- Motion/data rendering: Remotion 4.0.515

The atmosphere plate was generated with the built-in image generator using
this production prompt:

> A photorealistic macro scene of a generic ESP32-S3 development board connected
> by a black USB-C cable on a dark electronics workbench; board on the right,
> negative space on the left; low-key teal and warm amber rim lighting; premium
> cinematic product photography; no logos, readable labels, text, watermark,
> fantasy circuitry, hands, or impossible connectors.
