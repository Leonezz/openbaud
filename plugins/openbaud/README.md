# OpenBaud Codex plugin

This plugin bundles the OpenBaud MCP server and the workflow skill that teaches
Codex how to explore local serial hardware safely and preserve verified device
knowledge in the active project.

## Build the bundled runtime

From the repository root:

```sh
plugins/openbaud/scripts/build-runtime
plugins/openbaud/scripts/smoke-test
```

`build-runtime` builds for the current supported host, copies the binary under
`bin/<platform>/`, applies an ad-hoc signature on macOS for local testing, and
writes a SHA-256 sidecar. Set `OPENBAUD_CODESIGN_IDENTITY` to a Developer ID
Application identity for a distribution build. Notarization is a release step
and intentionally requires credentials outside this repository.

Release archives are built natively for macOS Apple Silicon, macOS Intel, Linux
x64/ARM64, and Windows x64. Each platform archive contains only its matching
runtime. The release workflow also aggregates the five verified runtimes into
one universal marketplace archive. Every package uses `launcher.mjs` to select
the runtime matching `process.platform` and `process.arch`.

## Install from Git

Register the OpenBaud Git marketplace once, then install the plugin:

```sh
codex plugin marketplace add Leonezz/openbaud --ref stable
codex plugin add openbaud@openbaud-marketplace
```

An HTTPS or SSH Git URL works as well. Private repositories require the user to
already have Git access. The release workflow advances `stable` only after all
five native packages and the universal bundle pass their smoke tests.

To refresh the marketplace immediately:

```sh
codex plugin marketplace upgrade openbaud-marketplace
```

Start a new Codex task after the upgrade so its MCP process and tool catalog use
the new version.

## Install a platform release archive

Download either the universal archive or the archive matching the host from the
GitHub Release and extract it. The extracted directory is a complete immutable
local marketplace, so it can be installed without Rust, Cargo, or another
binary download:

```sh
codex plugin marketplace add /absolute/path/to/openbaud-vMAJOR.MINOR.PATCH-universal
codex plugin add openbaud@openbaud-marketplace
```

Available platform names are `darwin-arm64`, `darwin-x64`, `linux-x64`,
`linux-arm64`, and `windows-x64`; `universal` contains all five.

## Install from a checkout

The source checkout tracks only the macOS ARM64 preview runtime. On another
supported host, run `plugins/openbaud/scripts/build-runtime` first to build that
host's runtime; otherwise use the `stable` branch or a release archive. Then
register the repository marketplace:

```sh
codex plugin marketplace add /absolute/path/to/openbaud
codex plugin add openbaud@openbaud-marketplace
```

Start a new Codex task after installation. The MCP process inherits that task's
working directory, so `devices/`, `captures/`, reports, and `.openbaud/` stay in
the user's project instead of the plugin cache.

## Release invariant

A published plugin must contain the runtime selected by `launcher.mjs` for its
supported host. Codex starts the Node launcher from the plugin root; OpenBaud
recovers the original task workspace from the inherited process environment.
Node.js 18 or newer is the only launcher prerequisite. Installing the plugin
never runs Homebrew, Cargo, npm lifecycle scripts, or an unreviewed download
hook.

The tag-triggered GitHub Actions release workflow builds five platform-specific
runtimes on native runners. It packages and smoke-tests each one, verifies all
checksums, aggregates them into a universal plugin, smoke-tests that plugin,
uploads all twelve assets to the matching GitHub Release, and advances the
`stable` branch to the exact universal marketplace contents.
