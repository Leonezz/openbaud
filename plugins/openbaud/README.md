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

The current release target is macOS Apple Silicon. The launcher already has
stable paths reserved for macOS Intel, Linux x64/ARM64, and Windows x64.

## Install from Git

Register the OpenBaud Git marketplace once, then install the plugin:

```sh
codex plugin marketplace add Leonezz/openbaud
codex plugin add openbaud@openbaud-marketplace
```

An HTTPS or SSH Git URL works as well. Private repositories require the user to
already have Git access. To pin a reproducible release rather than track the
default branch, add `--ref v0.1.2` to the marketplace command.

To refresh a marketplace that tracks a branch and reinstall its latest plugin:

```sh
codex plugin marketplace upgrade openbaud-marketplace
codex plugin add openbaud@openbaud-marketplace
```

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
