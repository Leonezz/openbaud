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
x64/ARM64, and Windows x64. Each archive contains only its matching runtime and
rewrites `.mcp.json` to launch that native executable directly.

## Install from Git

Register the OpenBaud Git marketplace once, then install the plugin:

```sh
codex plugin marketplace add Leonezz/openbaud
codex plugin add openbaud@openbaud-marketplace
```

An HTTPS or SSH Git URL works as well. Private repositories require the user to
already have Git access. To pin a reproducible release rather than track the
default branch, add `--ref v0.2.0` to the marketplace command.

To refresh a marketplace that tracks a branch and reinstall its latest plugin:

```sh
codex plugin marketplace upgrade openbaud-marketplace
codex plugin add openbaud@openbaud-marketplace
```

## Install a platform release archive

Download the archive matching the host from the GitHub Release and extract it.
The extracted directory is a complete local marketplace, so it can be installed
without Rust, Cargo, or another binary download:

```sh
codex plugin marketplace add /absolute/path/to/openbaud-v0.2.0-<platform>
codex plugin add openbaud@openbaud-marketplace
```

Available platform names are `darwin-arm64`, `darwin-x64`, `linux-x64`,
`linux-arm64`, and `windows-x64`.

## Install from a checkout

Register the repository marketplace once, then install the plugin:

```sh
codex plugin marketplace add /absolute/path/to/openbaud
codex plugin add openbaud@openbaud-marketplace
```

Start a new Codex task after installation. The MCP process inherits that task's
working directory, so `devices/`, `captures/`, reports, and `.openbaud/` stay in
the user's project instead of the plugin cache.

## Release invariant

A published plugin must contain the executable runtime named by `.mcp.json`.
Codex starts it from the plugin root; OpenBaud recovers the original task
workspace from the inherited process environment. Installing the plugin never
runs Homebrew, Cargo, npm lifecycle scripts, or an unreviewed download hook.

The tag-triggered GitHub Actions release workflow builds five platform-specific
runtimes on native runners. It packages each one as a self-contained local
marketplace, extracts it, runs the MCP smoke test, verifies its SHA-256 file,
and only then uploads all ten assets to the matching GitHub Release.
