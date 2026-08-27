import { useCallback, useEffect, useRef, useState } from 'react'
import {
  applyDocumentTheme,
  useApp,
  type App,
  type McpUiDisplayMode,
  type McpUiHostContext,
  type McpUiTheme,
  type McpUiToolInputNotification,
  type McpUiToolResultNotification,
} from '@modelcontextprotocol/ext-apps/react'

export type ToolArgs = McpUiToolInputNotification['params']['arguments']
/** Standard MCP CallToolResult as delivered by ui/notifications/tool-result. */
export type ToolResult = McpUiToolResultNotification['params']

/**
 * openbaud tool results: structuredContent carries the (possibly summarized)
 * result JSON. When the server spilled an oversized result to disk, the
 * summary keeps every key and adds `full_result` (workspace-relative path).
 */
export interface OpenbaudSummary {
  full_result?: string
  [key: string]: unknown
}

export function openbaudStructured(result: ToolResult): OpenbaudSummary | undefined {
  const structured = result.structuredContent
  if (structured === undefined || structured === null) return undefined
  return structured as OpenbaudSummary
}

// tokens.css keys dark on `.dark`, widget.css on [data-theme="dark"]; apply
// both so the two selector families never disagree.
function applyTheme(theme: McpUiTheme): void {
  applyDocumentTheme(theme)
  document.documentElement.classList.toggle('dark', theme === 'dark')
}

export interface UseWidgetOptions {
  name: string
  version: string
}

export interface WidgetHandle {
  app: App | null
  isConnected: boolean
  /** Initialization handshake failure, if any. */
  connectError: Error | null
  theme: McpUiTheme
  displayMode: McpUiDisplayMode | undefined
  hostContext: McpUiHostContext | undefined
  /** Latest complete tool input (ui/notifications/tool-input). */
  toolInput: ToolArgs
  /** Latest tool result (ui/notifications/tool-result). */
  toolResult: ToolResult | undefined
  /** Reason string when the host cancelled the running tool. */
  cancelReason: string | undefined
  callTool: (name: string, args?: Record<string, unknown>) => Promise<ToolResult>
  requestDisplayMode: (mode: McpUiDisplayMode) => Promise<McpUiDisplayMode>
}

/**
 * Project-wide MCP Apps bridge: connects via the official SDK, tracks tool
 * input/result notifications, and maps hostContext.theme onto the document
 * (data-theme attribute + `dark` class) for the two theme stylesheets.
 */
export function useWidget({ name, version }: UseWidgetOptions): WidgetHandle {
  const [theme, setTheme] = useState<McpUiTheme>('light')
  const [displayMode, setDisplayMode] = useState<McpUiDisplayMode | undefined>(undefined)
  const [hostContext, setHostContext] = useState<McpUiHostContext | undefined>(undefined)
  const [toolInput, setToolInput] = useState<ToolArgs>(undefined)
  const [toolResult, setToolResult] = useState<ToolResult | undefined>(undefined)
  const [cancelReason, setCancelReason] = useState<string | undefined>(undefined)

  const syncFromContext = useCallback((ctx: McpUiHostContext): void => {
    setHostContext({ ...ctx })
    if (ctx.theme) {
      applyTheme(ctx.theme)
      setTheme(ctx.theme)
    }
    if (ctx.displayMode) {
      setDisplayMode(ctx.displayMode)
    }
  }, [])

  const { app, isConnected, error } = useApp({
    appInfo: { name, version },
    capabilities: {},
    onAppCreated: (created) => {
      // Handlers must be registered before connect(): tool-input/tool-result
      // are one-shot notifications on strict hosts.
      created.addEventListener('toolinput', (params) => {
        setToolInput(params.arguments)
        setCancelReason(undefined)
      })
      created.addEventListener('toolresult', (params) => {
        setToolResult(params)
      })
      created.addEventListener('toolcancelled', (params) => {
        setCancelReason(params.reason ?? 'cancelled by host')
      })
      created.addEventListener('hostcontextchanged', () => {
        // The SDK merges the partial update into getHostContext() before
        // listeners fire; read the merged context.
        const merged = created.getHostContext()
        if (merged) syncFromContext(merged)
      })
    },
  })

  // Chrome tokens live under [data-theme]; give the document a ground state
  // so the card is styled while the handshake is still in flight.
  useEffect(() => {
    if (!document.documentElement.hasAttribute('data-theme')) {
      applyTheme('light')
    }
  }, [])

  useEffect(() => {
    if (!isConnected || !app) return
    const ctx = app.getHostContext()
    if (ctx) syncFromContext(ctx)
  }, [app, isConnected, syncFromContext])

  const appRef = useRef<App | null>(null)
  useEffect(() => {
    appRef.current = app
  }, [app])

  const callTool = useCallback(
    async (toolName: string, args?: Record<string, unknown>): Promise<ToolResult> => {
      const current = appRef.current
      if (!current) {
        throw new Error(`cannot call tool ${JSON.stringify(toolName)} — host not connected yet`)
      }
      return await current.callServerTool({ name: toolName, arguments: args })
    },
    [],
  )

  const requestDisplayMode = useCallback(
    async (mode: McpUiDisplayMode): Promise<McpUiDisplayMode> => {
      const current = appRef.current
      if (!current) {
        throw new Error(`cannot request display mode ${JSON.stringify(mode)} — host not connected yet`)
      }
      const result = await current.requestDisplayMode({ mode })
      setDisplayMode(result.mode)
      return result.mode
    },
    [],
  )

  return {
    app,
    isConnected,
    connectError: error,
    theme,
    displayMode,
    hostContext,
    toolInput,
    toolResult,
    cancelReason,
    callTool,
    requestDisplayMode,
  }
}
