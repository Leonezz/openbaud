# openbaud widgets

MCP Apps widget UIs for the openbaud MCP server: `port-picker` (serial port
selection) and `viewer` (live device output). React 19 + TypeScript +
`@modelcontextprotocol/ext-apps`, built with Vite + `vite-plugin-singlefile`
into self-contained single-file HTML (no external requests) that the Rust
server embeds from `crates/openbaud/src/mcp/ui/`.

## Commands

```sh
pnpm install --frozen-lockfile
pnpm harness    # dev harness (Vite) with fixture data, hot reload
pnpm typecheck  # tsc --noEmit
pnpm build      # emits dist/<app>.html single files
```

## Rust build prerequisite

`cargo build` embeds the HTML files under `crates/openbaud/src/mcp/ui/`, so run
`pnpm build` here (and copy `dist/port-picker.html` + `dist/viewer.html` there)
at least once before building the crate after changing widget sources.
`plugins/openbaud/scripts/build-runtime` does both automatically.
