// Hand-written MCP Apps host per spec 2026-01-26 — deliberately NOT using the
// SDK's AppBridge, so this fixture exercises the wire protocol independently
// of the library under test. JSON-RPC 2.0 over window.postMessage.
import {
  LIST_PORTS_RESULT,
  OPEN_EBUSY_MESSAGE,
  OPEN_OK_RESULT,
  radarCommandResult,
  radarScanCount,
  wrapToolError,
  wrapToolResult,
  type JsonObject,
} from './fixtures'

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
const openModeSel = el<HTMLSelectElement>('#open-mode')

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
    case 'list_ports':
      respond(id, wrapToolResult(LIST_PORTS_RESULT))
      break
    case 'open':
      if (openModeSel.value === 'ebusy') {
        respond(id, wrapToolError(OPEN_EBUSY_MESSAGE))
      } else {
        respond(id, wrapToolResult(OPEN_OK_RESULT))
      }
      break
    case 'run_command':
      respond(id, wrapToolResult(radarCommandResult(radarIndex++)))
      break
    default:
      respond(
        id,
        wrapToolError(`no canned result for tool ${JSON.stringify(name)} — add one to harness/fixtures.ts`),
      )
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
        hostCapabilities: { serverTools: {}, logging: {} },
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
    case 'ping':
      respond(id, {})
      break
    default:
      respondError(id, -32601, `harness does not implement ${msg.method}`)
  }
}

function sendScenario(): void {
  if (scenarioSel.value === 'list_ports') {
    notify('ui/notifications/tool-input', { arguments: {} })
    notify('ui/notifications/tool-result', wrapToolResult(LIST_PORTS_RESULT))
  } else {
    notify('ui/notifications/tool-input', {
      arguments: { device: 'openbaud-pv-board', command: 'obp1_radar_scan', params: { seq: 42 } },
    })
    notify('ui/notifications/tool-result', wrapToolResult(radarCommandResult(radarIndex++)))
  }
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

el<HTMLButtonElement>('#next-frame').addEventListener('click', () => {
  notify('ui/notifications/tool-result', wrapToolResult(radarCommandResult(radarIndex++)))
  log('harness', `sent radar frame ${((radarIndex - 1) % radarScanCount()) + 1}/${radarScanCount()}`)
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
