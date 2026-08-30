// Hand-written MCP Apps host per spec 2026-01-26 — deliberately NOT using the
// SDK's AppBridge, so this fixture exercises the wire protocol independently
// of the library under test. JSON-RPC 2.0 over window.postMessage.
import {
  aliasScanResult,
  ASK_PORT_INPUT,
  ASK_PORT_RESULT,
  brokenViewResult,
  LIST_PORTS_RESULT,
  radarCommandResult,
  radarScanCount,
  replayRadarResult,
  resultTextForUri,
  showResultEnvelope,
  SHOW_RESULT_INLINE,
  SHOW_RESULT_INLINE_ENCODED,
  undeclaredCommandResult,
  wrapToolError,
  wrapToolResult,
  type JsonObject,
} from './fixtures'
import { captureEnvelope, captureTextForUri } from './capture-fixture'
import { diagnosticsResult } from './diagnostics-fixture'
import { timelineEnvelope, timelineTextForUri } from './timeline-fixture'

type Theme = 'light' | 'dark'

interface JsonRpcMessage {
  jsonrpc: '2.0'
  id?: number | string
  method?: string
  params?: unknown
  result?: unknown
  error?: unknown
}

const PROTOCOL_VERSION = '2026-01-26'

function el<T extends HTMLElement>(selector: string): T {
  const found = document.querySelector<T>(selector)
  if (!found) {
    throw new Error(`harness: missing element ${selector}`)
  }
  return found
}

const frame = el<HTMLIFrameElement>('#frame')
const logEl = el<HTMLPreElement>('#log')
const appUrlInput = el<HTMLInputElement>('#app-url')
const scenarioSel = el<HTMLSelectElement>('#scenario')
const ctxModeSel = el<HTMLSelectElement>('#ctx-mode')
const resourceModeSel = el<HTMLSelectElement>('#resource-mode')

/** Delay before a slow resources/read answers — long enough to see the skeleton. */
const SLOW_RESOURCE_MS = 2500

let theme: Theme = 'light'
let radarIndex = 0

function log(tag: string, payload: unknown): void {
  const time = new Date().toISOString().slice(11, 23)
  const body = typeof payload === 'string' ? payload : JSON.stringify(payload)
  logEl.textContent += `[${time}] ${tag} ${body}\n`
  logEl.scrollTop = logEl.scrollHeight
}

function post(msg: JsonRpcMessage): void {
  const target = frame.contentWindow
  if (!target) {
    throw new Error('harness: iframe has no contentWindow — load an app first')
  }
  target.postMessage(msg, '*')
  log('host→app', msg)
}

function respond(id: number | string, result: unknown): void {
  post({ jsonrpc: '2.0', id, result })
}

function respondError(id: number | string, code: number, message: string): void {
  post({ jsonrpc: '2.0', id, error: { code, message } })
}

function notify(method: string, params: unknown): void {
  post({ jsonrpc: '2.0', method, params: params as JsonObject })
}

function hostContext(): JsonObject {
  return {
    theme,
    displayMode: 'inline',
    availableDisplayModes: ['inline', 'fullscreen'],
    platform: 'web',
    locale: 'en-US',
    timeZone: 'UTC',
    userAgent: 'openbaud-harness/0.0.0',
  }
}

function handleToolsCall(id: number | string, params: JsonObject): void {
  const name = typeof params.name === 'string' ? params.name : undefined
  switch (name) {
    // list_ports has no UI binding, so the picker may call it for a rescan.
    case 'list_ports':
      respond(id, wrapToolResult(LIST_PORTS_RESULT))
      break
    case 'ask_port':
      respond(id, wrapToolResult(ASK_PORT_RESULT))
      break
    // No case for `open` or the other data tools (run_command, read,
    // request…): under the 2026-08 contract only the agent calls those.
    default:
      respond(
        id,
        wrapToolError(`no canned result for tool ${JSON.stringify(name)} — add one to harness/fixtures.ts`),
      )
  }
}

/** MCP resources/read: serves the full saved result show_result pointed at. */
function handleResourcesRead(id: number | string, params: JsonObject): void {
  const uri = typeof params.uri === 'string' ? params.uri : ''
  if (resourceModeSel.value === 'fail') {
    respondError(id, -32002, `resource ${uri} not found (harness: resource mode = fail)`)
    return
  }
  const text = timelineTextForUri(uri) ?? captureTextForUri(uri) ?? resultTextForUri(uri)
  if (text === undefined) {
    respondError(id, -32002, `harness has no fixture for resource ${JSON.stringify(uri)}`)
    return
  }
  const send = (): void =>
    respond(id, { contents: [{ uri, mimeType: 'application/json', text }] })
  if (resourceModeSel.value === 'slow') {
    log('harness', `delaying resources/read ${uri} by ${SLOW_RESOURCE_MS} ms`)
    window.setTimeout(send, SLOW_RESOURCE_MS)
  } else {
    send()
  }
}

function handleRequest(msg: JsonRpcMessage): void {
  const id = msg.id
  if (id === undefined) return
  switch (msg.method) {
    case 'ui/initialize':
      respond(id, {
        protocolVersion: PROTOCOL_VERSION,
        hostInfo: { name: 'openbaud-harness', version: '0.0.0' },
        hostCapabilities: {
          serverTools: {},
          serverResources: {},
          logging: {},
          updateModelContext: { text: {}, structuredContent: {} },
        },
        hostContext: hostContext(),
      })
      break
    case 'ui/request-display-mode': {
      const mode = (msg.params as { mode?: string } | undefined)?.mode ?? 'inline'
      respond(id, { mode })
      notify('ui/notifications/host-context-changed', { displayMode: mode })
      break
    }
    case 'tools/call':
      handleToolsCall(id, (msg.params ?? {}) as JsonObject)
      break
    case 'resources/read':
      handleResourcesRead(id, (msg.params ?? {}) as JsonObject)
      break
    case 'ui/update-model-context':
      // What the agent would receive; logged in full so the payload the
      // picker pushes can be inspected.
      log('model-context', JSON.stringify(msg.params, null, 2))
      if (ctxModeSel.value === 'reject') {
        respondError(id, -32001, 'harness: model context update rejected (ctx mode = reject)')
      } else {
        respond(id, {})
      }
      break
    case 'ping':
      respond(id, {})
      break
    default:
      respondError(id, -32601, `harness does not implement ${msg.method}`)
  }
}

/** The inline show_result scenarios: { data, encoding? } in, envelope out. */
const INLINE_SCENARIOS: Readonly<
  Record<string, { readonly data: (index: number) => JsonObject; readonly envelope: JsonObject }>
> = {
  // Payload declares its own `view` (angle_deg / distance_mm).
  show_result_inline: { data: radarCommandResult, envelope: SHOW_RESULT_INLINE },
  // No `view`; show_result's `encoding` names foreign fields instead.
  show_result_encoded: { data: aliasScanResult, envelope: SHOW_RESULT_INLINE_ENCODED },
  // `view.angle` names a field the records do not carry — expect .ob-error.
  show_result_broken: { data: brokenViewResult, envelope: SHOW_RESULT_INLINE },
  // Nothing declared anywhere — expect the generic key/value card.
  show_result_undeclared: { data: undeclaredCommandResult, envelope: SHOW_RESULT_INLINE },
  // `port` names a replay transport — expect REPLAY badge + scope watermark.
  show_result_replay: { data: replayRadarResult, envelope: SHOW_RESULT_INLINE },
  // diagnose_frame payload: real OBP frame with its CRC rewritten to ccitt.
  show_result_diagnostics: { data: () => diagnosticsResult(), envelope: SHOW_RESULT_INLINE },
}

/** show_result for scan `index`: the envelope only — the payload is a resource. */
function sendShowResult(index: number): void {
  if (scenarioSel.value === 'show_result_timeline' || scenarioSel.value === 'show_result_capture') {
    // Timeline / capture slice: envelope only; the full JSON rides resources/read.
    const envelope =
      scenarioSel.value === 'show_result_timeline' ? timelineEnvelope() : captureEnvelope()
    notify('ui/notifications/tool-input', { arguments: { path: envelope.path } })
    notify('ui/notifications/tool-result', wrapToolResult(envelope))
    return
  }
  const inline = INLINE_SCENARIOS[scenarioSel.value]
  if (inline !== undefined) {
    notify('ui/notifications/tool-input', { arguments: { data: inline.data(index) } })
    notify('ui/notifications/tool-result', wrapToolResult(inline.envelope))
    return
  }
  const envelope = showResultEnvelope(index)
  notify('ui/notifications/tool-input', { arguments: { path: envelope.path } })
  notify('ui/notifications/tool-result', wrapToolResult(envelope))
}

function sendScenario(): void {
  if (scenarioSel.value === 'tool_cancelled') {
    // Host cancels the call after the input: tool-input, then tool-cancelled,
    // deliberately NO tool-result — the app's cancelled branch must render.
    notify('ui/notifications/tool-input', { arguments: ASK_PORT_INPUT })
    notify('ui/notifications/tool-cancelled', {
      reason: 'harness: host cancelled the tool call (scenario tool_cancelled)',
    })
    return
  }
  if (scenarioSel.value.startsWith('show_result')) {
    sendShowResult(radarIndex++)
    return
  }
  notify('ui/notifications/tool-input', { arguments: ASK_PORT_INPUT })
  notify('ui/notifications/tool-result', wrapToolResult(ASK_PORT_RESULT))
}

window.addEventListener('message', (event: MessageEvent) => {
  if (event.source !== frame.contentWindow) return
  const msg = event.data as JsonRpcMessage
  if (!msg || msg.jsonrpc !== '2.0') return
  log('app→host', msg)
  if (msg.method !== undefined && msg.id !== undefined) {
    handleRequest(msg)
  } else if (msg.method === 'ui/notifications/initialized') {
    log('harness', `app initialized — auto-sending scenario "${scenarioSel.value}"`)
    sendScenario()
  }
  // Remaining notifications (size-changed, notifications/message, …) are only logged.
})

el<HTMLButtonElement>('#load').addEventListener('click', () => {
  radarIndex = 0
  logEl.textContent = ''
  log('harness', `loading ${appUrlInput.value}`)
  frame.src = appUrlInput.value
})

el<HTMLButtonElement>('#send-scenario').addEventListener('click', sendScenario)

el<HTMLButtonElement>('#next-result').addEventListener('click', () => {
  sendShowResult(radarIndex++)
  log('harness', `sent saved result ${((radarIndex - 1) % radarScanCount()) + 1}/${radarScanCount()}`)
})

el<HTMLButtonElement>('#toggle-theme').addEventListener('click', () => {
  theme = theme === 'light' ? 'dark' : 'light'
  document.documentElement.dataset.theme = theme
  notify('ui/notifications/host-context-changed', { theme })
})

const initialApp = new URLSearchParams(location.search).get('app') ?? '/src/apps/_smoke/index.html'
appUrlInput.value = initialApp
frame.src = initialApp
log('harness', `loading ${initialApp}`)
