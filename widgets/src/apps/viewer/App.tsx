import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { Card } from '../../components/Card'
import { ObError } from '../../components/ObError'
import { ObLoading } from '../../components/ObLoading'
import { useWidget, type ToolResult } from '../../mcp/useWidget'
import { inferToolName } from './dispatch'
import { GenericView } from './GenericView'
import { PolarView } from './PolarView'
import { failWith, INITIAL_VIEWER, ingestResult, type ViewerState } from './state'

function ShellCard({ children }: { children: ReactNode }) {
  return (
    <div className="viewer-root">
      <Card title="openbaud viewer">{children}</Card>
    </div>
  )
}

/** Root: SDK bridge in, schema dispatch out (polar radar vs generic KV card). */
export function ViewerApp() {
  const widget = useWidget({ name: 'openbaud-viewer', version: '0.1.0' })
  const [state, setState] = useState<ViewerState>(INITIAL_VIEWER)

  const ingest = useCallback((result: ToolResult): void => {
    setState((prev) => ingestResult(prev, result))
  }, [])

  const fail = useCallback((message: string): void => {
    setState((prev) => failWith(prev, message))
  }, [])

  // Ref guard keeps StrictMode's double effect run from ingesting one result
  // twice (which would fold the frame into its own ghost).
  const lastIngested = useRef<ToolResult | undefined>(undefined)
  useEffect(() => {
    const result = widget.toolResult
    if (result === undefined || result === lastIngested.current) return
    lastIngested.current = result
    ingest(result)
  }, [widget.toolResult, ingest])

  const toolName = inferToolName(widget.hostContext?.toolInfo?.tool.name, widget.toolInput)

  const retry = useCallback(async (): Promise<void> => {
    if (toolName === undefined) return
    try {
      ingest(await widget.callTool(toolName, widget.toolInput))
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      fail(`tool call failed: ${message}`)
    }
  }, [widget, toolName, ingest, fail])

  if (widget.connectError !== null) {
    return (
      <ShellCard>
        <ObError
          title="Host connection failed"
          detail={`${widget.connectError.message} — run inside the harness (pnpm harness) or an MCP Apps host.`}
        />
      </ShellCard>
    )
  }
  if (!widget.isConnected) {
    return (
      <ShellCard>
        <ObLoading />
      </ShellCard>
    )
  }

  switch (state.kind) {
    case 'idle':
      if (widget.cancelReason !== undefined) {
        return (
          <ShellCard>
            <ObError title="Tool call cancelled" detail={widget.cancelReason} />
          </ShellCard>
        )
      }
      return (
        <ShellCard>
          <ObLoading />
        </ShellCard>
      )
    case 'error':
      return (
        <ShellCard>
          <ObError
            title="Tool call failed"
            detail={state.message}
            onRetry={toolName !== undefined ? () => void retry() : undefined}
          />
        </ShellCard>
      )
    case 'generic':
      return <GenericView structured={state.structured} toolName={toolName} />
    case 'polar':
      return (
        <PolarView
          widget={widget}
          radar={state.radar}
          toolName={toolName}
          toolArgs={widget.toolInput}
          onResult={ingest}
          onFailure={fail}
        />
      )
  }
}
