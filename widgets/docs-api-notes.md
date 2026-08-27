# @modelcontextprotocol/ext-apps — verified API notes (v1.7.5)

Facts below were read from the installed package's `.d.ts` files
(`node_modules/@modelcontextprotocol/ext-apps/dist/src/`), not from memory.
Re-verify against the `.d.ts` after any dependency bump.

## Package exports (package.json `exports`)

| Specifier | Types |
|---|---|
| `@modelcontextprotocol/ext-apps` | `dist/src/app.d.ts` |
| `@modelcontextprotocol/ext-apps/react` | `dist/src/react/index.d.ts` (re-exports everything from `../app`) |
| `@modelcontextprotocol/ext-apps/app-bridge` | host-side `AppBridge` (we hand-write our harness host instead) |
| `@modelcontextprotocol/ext-apps/server` | server helpers (`registerAppTool`, `registerAppResource`) |

`LATEST_PROTOCOL_VERSION = "2026-01-26"`.

## Core class: `App` (extends `ProtocolWithEvents`)

```ts
new App(appInfo: Implementation, capabilities?: McpUiAppCapabilities, options?: AppOptions)
// AppOptions: { autoResize?: boolean /* default true */, strict?: boolean, allowUnsafeEval?: boolean }
await app.connect(transport?: Transport, options?: RequestOptions): Promise<void>
// default transport: new PostMessageTransport(window.parent, window.parent)
```

`connect()` sends `ui/initialize` → waits for result → sends
`ui/notifications/initialized` → (autoResize) starts ResizeObserver
size-changed notifications.

### Getters (valid after connect)

- `app.getHostCapabilities(): McpUiHostCapabilities | undefined` — `serverTools`, `openLinks`, `downloadFile`, `sampling`, `message`, `updateModelContext`, `logging`
- `app.getHostVersion(): Implementation | undefined`
- `app.getHostContext(): McpUiHostContext | undefined` — `theme` (`"light" | "dark"`), `styles`, `displayMode`, `availableDisplayModes`, `containerDimensions`, `locale`, `timeZone`, `platform`, `toolInfo: { id?, tool }`

### Events (DOM-model, typed via `AppEventMap`)

`app.addEventListener(name, handler)` / `removeEventListener` — preferred.
Deprecated equivalent `on*` setters exist (`ontoolinput`, `ontoolinputpartial`,
`ontoolresult`, `ontoolcancelled`, `onhostcontextchanged`).

| Event | Params type | Notification |
|---|---|---|
| `toolinput` | `{ arguments?: Record<string, unknown> }` | `ui/notifications/tool-input` |
| `toolinputpartial` | same, healed/partial JSON | `ui/notifications/tool-input-partial` |
| `toolresult` | `CallToolResult` (`content`, `structuredContent?`, `isError?`) | `ui/notifications/tool-result` |
| `toolcancelled` | `{ reason?: string }` | `ui/notifications/tool-cancelled` |
| `hostcontextchanged` | partial `McpUiHostContext` (merged into `getHostContext()` before handlers fire) | `ui/notifications/host-context-changed` |

Register handlers BEFORE `connect()` — tool-input/tool-result are one-shot;
the SDK warns (or throws under `strict`) on late first registration.

### Host-bound requests (methods, exact names)

- `app.callServerTool(params: { name, arguments? }, options?): Promise<CallToolResult>` — transport failures throw; tool failures return `isError: true`.
- `app.readServerResource({ uri }): Promise<ReadResourceResult>`
- `app.listServerResources(params?): Promise<ListResourcesResult>`
- `app.requestDisplayMode({ mode: "inline" | "fullscreen" | "pip" }): Promise<{ mode }>` — result carries the mode actually set.
- `app.sendMessage({ role, content }): Promise<{ isError?: boolean }>`
- `app.updateModelContext({ content?, structuredContent? })`
- `app.openLink({ url })`, `app.downloadFile({ contents })`
- `app.createSamplingMessage(params)` (check `getHostCapabilities()?.sampling`)
- `app.sendLog({ level, data, logger? })`, `app.sendSizeChanged({ width?, height? })`, `app.requestTeardown()`
- Handlers the app can expose: `app.onteardown`, `app.oncalltool`, `app.onlisttools`, `app.registerTool(name, config, cb)`

## React hooks (`@modelcontextprotocol/ext-apps/react`)

- `useApp({ appInfo, capabilities, onAppCreated?, autoResize?, strict? }): { app: App | null, isConnected: boolean, error: Error | null }`
  - Options are read on first mount only; register event handlers inside `onAppCreated`.
- `useDocumentTheme(): "light" | "dark"` — reactive via MutationObserver on `data-theme` / `class`.
- `useHostStyleVariables(app, initialContext?)` — applies `styles.variables` + `color-scheme`.
- `useHostFonts(app, initialContext?)` — injects `styles.css.fonts`.
- `useAutoResize(app)` — only needed when `autoResize: false`.

## Style helpers (also exported from root)

- `applyDocumentTheme(theme)` — sets `data-theme` attribute + `style.colorScheme` on `document.documentElement`. Our `useWidget` additionally mirrors the theme onto the `dark` class so the generated `tokens.css` `.dark` block matches too.
- `getDocumentTheme()` — reads `data-theme`, falls back to `.dark` class.
- `applyHostStyleVariables(styles, root?)`, `applyHostFonts(fontCss)`

## Host side (for the harness; hand-written per spec 2026-01-26)

JSON-RPC 2.0 over `window.postMessage`. Message flow the harness must implement:

1. answer request `ui/initialize` → result `{ protocolVersion, hostInfo, hostCapabilities, hostContext }`
2. receive notification `ui/notifications/initialized`
3. send notifications `ui/notifications/tool-input` / `ui/notifications/tool-result` / `ui/notifications/host-context-changed`
4. answer app requests: `tools/call` (canned results), `ui/request-display-mode` → `{ mode }`
5. receive `ui/notifications/size-changed`

## openbaud server result shapes (from crates/openbaud/src/mcp/)

`tools/call` responses wrap the tool result as
`{ content: [{ type: "text", text: <pretty JSON> }], structuredContent: <result> }`;
errors as `{ content: [{ type: "text", text: "error: …" }], isError: true }`.
Results larger than `max_inline_bytes` (default 4096) are summarized and gain a
top-level `full_result: ".openbaud/out/res-<ms>-<tool>.json"` (see `output.rs`:
strings cut at 256 chars with `…(N bytes total)`, arrays >16 keep 8 elements +
`{"truncated": N}`).
