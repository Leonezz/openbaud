# OpenBaud

OpenBaud gives Codex structured, audited access to local serial hardware. It
combines an MCP server with durable device profiles, typed commands, workflows,
lossless captures, replay, and a hardware-exploration skill.

## Install the Codex plugin

Register this Git repository as the `openbaud` marketplace and install its
plugin:

```sh
codex plugin marketplace add Leonezz/openbaud
codex plugin add openbaud@openbaud-marketplace
```

Start a new Codex task after installation, then try:

```text
Use OpenBaud to list the serial devices connected to this computer.
```

The plugin bundles its native MCP runtime; users do not need Rust, Cargo,
Homebrew, or an install hook. The v0.1.1 bundle currently supports macOS Apple
Silicon. The binary is ad-hoc signed for Git-based preview distribution but is
not yet Apple-notarized.

To pin the current release rather than follow the default branch:

```sh
codex plugin marketplace add Leonezz/openbaud --ref v0.1.1
codex plugin add openbaud@openbaud-marketplace
```

Private repositories require existing Git credentials. A marketplace that
tracks a branch can be refreshed with:

```sh
codex plugin marketplace upgrade openbaud-marketplace
codex plugin add openbaud@openbaud-marketplace
```

### Upgrade from v0.1.0

v0.1.1 gives the Marketplace and Plugin distinct identifiers. This avoids an
ambiguous repeated cache path in Codex while keeping the user-facing name
OpenBaud:

```sh
codex plugin remove openbaud@openbaud
codex plugin marketplace remove openbaud
codex plugin marketplace add Leonezz/openbaud --ref v0.1.1
codex plugin add openbaud@openbaud-marketplace
```

## Build and verify

```sh
cargo test --workspace
plugins/openbaud/scripts/build-runtime
plugins/openbaud/scripts/smoke-test
```

See [the plugin README](plugins/openbaud/README.md) for packaging details and
platform paths.

## Safety

Serial writes can reconfigure or actuate physical hardware. OpenBaud marks raw
writes and device commands as potentially destructive MCP operations, audits
write attempts, and requires explicit acknowledgement for commands declared
with `risk: danger`.

## License

MIT
