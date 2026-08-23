# OpenBaud

[![CI](https://github.com/Leonezz/openbaud/actions/workflows/ci.yml/badge.svg)](https://github.com/Leonezz/openbaud/actions/workflows/ci.yml)

OpenBaud gives Codex structured, audited access to local serial hardware. It
combines an MCP server with durable device profiles, typed commands, workflows,
lossless captures, replay, and a hardware-exploration skill.

## Install the Codex plugin

Register this Git repository as the `openbaud-marketplace` marketplace and
install its plugin:

```sh
codex plugin marketplace add Leonezz/openbaud
codex plugin add openbaud@openbaud-marketplace
```

Start a new Codex task after installation, then try:

```text
Use OpenBaud to list the serial devices connected to this computer.
```

The plugin bundles its native MCP runtime; users do not need Rust, Cargo,
Homebrew, or an install hook. The Git marketplace checkout currently bundles
macOS Apple Silicon. The v0.1.4 GitHub Release additionally publishes
self-contained marketplace archives for macOS Intel, Linux x64/ARM64, and
Windows x64. macOS binaries are ad-hoc signed for preview distribution but are
not yet Apple-notarized.

To pin the current release rather than follow the default branch:

```sh
codex plugin marketplace add Leonezz/openbaud --ref v0.1.4
codex plugin add openbaud@openbaud-marketplace
```

Private repositories require existing Git credentials. A marketplace that
tracks a branch can be refreshed with:

```sh
codex plugin marketplace upgrade openbaud-marketplace
codex plugin add openbaud@openbaud-marketplace
```

### Upgrade from v0.1.0

The marketplace and plugin now use distinct identifiers. Remove the original
ambiguous installation before installing v0.1.4:

```sh
codex plugin remove openbaud@openbaud
codex plugin marketplace remove openbaud
codex plugin marketplace add Leonezz/openbaud --ref v0.1.4
codex plugin add openbaud@openbaud-marketplace
```

### Upgrade from v0.1.1 or v0.1.2

Re-register the pinned marketplace to use v0.1.4:

```sh
codex plugin remove openbaud@openbaud-marketplace
codex plugin marketplace remove openbaud-marketplace
codex plugin marketplace add Leonezz/openbaud --ref v0.1.4
codex plugin add openbaud@openbaud-marketplace
```

## Build and verify

```sh
cargo test --workspace
python3 scripts/validate-package.py
plugins/openbaud/scripts/build-runtime
plugins/openbaud/scripts/smoke-test
```

Repository-level agent instructions live at
`.agents/skills/openbaud/SKILL.md`. `openbaud init` writes the same general
Agent Skills path into new workspaces; the Codex plugin keeps its packaged copy
under `plugins/openbaud/skills/` as required by the plugin format.

GitHub Actions runs Clippy, package validation, and native build/test jobs for
macOS ARM64, macOS Intel, Linux x64, Linux ARM64, and Windows x64. Every job
builds a platform archive, unpacks it, and performs a real MCP smoke test.
Pushing a stable `vMAJOR.MINOR.PATCH` tag repeats those release gates and
publishes five native, self-contained marketplace archives plus SHA-256 files.

See [the plugin README](plugins/openbaud/README.md) for packaging details and
platform paths.

## Safety

Serial writes can reconfigure or actuate physical hardware. OpenBaud marks raw
writes and device commands as potentially destructive MCP operations, audits
write attempts, and requires explicit acknowledgement for commands declared
with `risk: danger`.

## License

MIT
