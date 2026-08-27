import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { Card } from '../../components/Card'
import { ObError } from '../../components/ObError'
import { ObLoading } from '../../components/ObLoading'
import type { OpenbaudSummary, ToolArgs, ToolResult } from '../../mcp/useWidget'
import { useWidget } from '../../mcp/useWidget'
import { isRecord } from './dispatch'
import { GenericView } from './GenericView'
import { PolarView } from './PolarView'
import {
  INITIAL_VIEWER,
  parseResourceJson,
  readEnvelope,
  viewFor,
  type ViewerState,
} from './state'

function ShellCard({ children }: { children: ReactNode }) {
  return (
    <div className="viewer-root">
      <Card title="openbaud viewer">{children}</Card>
    </div>
  )
}

/**
 * Root of the show_result viewer.
 *
 * This widget is bound to show_result — a tool whose whole purpose is display —
 * and it never calls a data tool itself. What it renders is one result the
 * server already produced and saved, so there is no scan loop, no poll rate and
 * no previous frame to fade: live observation is a stream-subscription route
 * that does not exist yet, and faking it here would misreport stale data as
 * live. The tool result is only a reference; the payload is pulled from the
 * openbaud://result/… resource so the model's context stays small.
 */
export function ViewerApp() {
  const widget = useWidget({ name: 'openbaud-viewer', version: '0.1.0' })
  const [state, setState] = useState<ViewerState>(INITIAL_VIEWER)

  const resolve = useCallback(
    async (result: ToolResult, args: ToolArgs): Promise<void> => {
      const envelope = readEnvelope(result)
      if (envelope.kind === 'failed') {
        setState({ kind: 'error', message: envelope.message })
        return
      }
      const { ref } = envelope
      if (ref.source === 'inline') {
        // Small results never hit disk; show_result echoes them in its own input.
        const data = args?.data
        if (!isRecord(data)) {
          setState({
            kind: 'error',
            message:
              'show_result reported source "inline" but its tool input carried no `data` object — nothing to render',
          })
          return
        }
        setState(viewFor(data as OpenbaudSummary, ref))
        return
      }
      setState({ kind: 'loading', ref })
      try {
        const read = await widget.readResource(ref.uri)
        setState(viewFor(parseResourceJson(read, ref.uri), ref))
      } catch (error) {
        const detail = error instanceof Error ? error.message : String(error)
        setState({ kind: 'error', message: `could not read ${ref.uri} — ${detail}` })
      }
    },
    [widget],
  )

  // tool-input lands one notification before tool-result; keep the latest in a
  // ref so resolving a result never re-runs on unrelated re-renders.
  const argsRef = useRef<ToolArgs>(undefined)
  useEffect(() => {
    argsRef.current = widget.toolInput
  }, [widget.toolInput])

  // Ref guard keeps StrictMode's double effect run from fetching the same
  // resource twice.
  const resolved = useRef<ToolResult | undefined>(undefined)
  useEffect(() => {
    const result = widget.toolResult
    if (result === undefined || result === resolved.current) return
    resolved.current = result
    void resolve(result, argsRef.current)
  }, [widget.toolResult, resolve])

  const retry = useCallback((): void => {
    const result = widget.toolResult
    if (result === undefined) return
    void resolve(result, argsRef.current)
  }, [widget.toolResult, resolve])

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
    case 'loading':
      return (
        <ShellCard>
          <ObLoading />
          <div className="viewer-kpi">
            <span>
              reading <b>{state.ref.source === 'file' ? state.ref.uri : 'inline result'}</b>
            </span>
          </div>
        </ShellCard>
      )
    case 'error':
      return (
        <ShellCard>
          <ObError
            title="Result unavailable"
            detail={state.message}
            onRetry={widget.toolResult !== undefined ? retry : undefined}
          />
        </ShellCard>
      )
    case 'generic':
      return <GenericView structured={state.structured} resultRef={state.ref} />
    case 'polar':
      return <PolarView widget={widget} scan={state.scan} resultRef={state.ref} />
  }
}
