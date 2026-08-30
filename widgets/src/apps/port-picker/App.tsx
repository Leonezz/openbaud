import { useCallback, useEffect, useState } from 'react'
import { Btn } from '../../components/Btn'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { ObEmpty } from '../../components/ObEmpty'
import { ObError } from '../../components/ObError'
import { ObLoading } from '../../components/ObLoading'
import { useWidget, type ToolResult } from '../../mcp/useWidget'
import { LoadingFoot, PickerFoot, RescanFoot, type SendState } from './Foot'
import { PortTable } from './PortTable'
import {
  aliasesByCanonical,
  findPort,
  firstSelectable,
  isBlocked,
  parseAskPort,
  parseListPorts,
  portSelection,
  selectionText,
  toolResultText,
  visiblePorts,
  type EnrichedPort,
} from './ports'

/** ask_port (tool result) or list_ports (Rescan) candidates. */
type Load =
  | { readonly kind: 'waiting' }
  | { readonly kind: 'failed'; readonly title: string; readonly message: string }
  | { readonly kind: 'ready'; readonly candidates: readonly EnrichedPort[] }

/** Why the agent asked, and for which workspace device. */
interface AskMeta {
  readonly reason?: string
  readonly device?: string
}

const NO_PORTS: readonly EnrichedPort[] = []

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

// The toggle is the only place aliases are named, and it says what clicking
// does. A per-row "N aliases hidden" badge was removed: a count the reader
// cannot act on is not information.
function aliasLabel(count: number, shown: boolean): string {
  const noun = count === 1 ? '1 duplicate node' : `${count} duplicate nodes`
  return shown ? `Hide ${noun}` : `Show ${noun}`
}

export function PortPickerApp() {
  const widget = useWidget({ name: 'openbaud-port-picker', version: '0.2.0' })
  const { callTool, updateModelContext, toolResult, cancelReason } = widget

  const [load, setLoad] = useState<Load>({ kind: 'waiting' })
  const [meta, setMeta] = useState<AskMeta>({})
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [showAliases, setShowAliases] = useState(false)
  const [send, setSend] = useState<SendState>({ kind: 'idle' })

  // `device` is who the agent asked for (tool input `device`, echoed in the
  // ask_port result) — it steers preselection toward that device's own port.
  const acceptCandidates = useCallback(
    (candidates: readonly EnrichedPort[], device: string | undefined): void => {
      setSend({ kind: 'idle' })
      setShowAliases(false)
      setSelectedPath(firstSelectable(visiblePorts(candidates, false), device))
      setLoad({ kind: 'ready', candidates })
    },
    [],
  )

  const fail = useCallback((title: string, message: string): void => {
    setSelectedPath(null)
    setLoad({ kind: 'failed', title, message })
  }, [])

  // ask_port arrives as the tool result the template is bound to.
  const applyAskResult = useCallback(
    (result: ToolResult): void => {
      if (result.isError) {
        fail('ask_port failed', toolResultText(result) || 'ask_port returned an error')
        return
      }
      try {
        const ask = parseAskPort(result.structuredContent)
        setMeta({ reason: ask.reason, device: ask.device })
        acceptCandidates(ask.candidates, ask.device)
      } catch (error) {
        fail('Unreadable ask_port result', errorMessage(error))
      }
    },
    [acceptCandidates, fail],
  )

  useEffect(() => {
    if (toolResult !== undefined) applyAskResult(toolResult)
  }, [toolResult, applyAskResult])

  useEffect(() => {
    if (cancelReason === undefined) return
    setLoad((prev) =>
      prev.kind === 'waiting'
        ? { kind: 'failed', title: 'Cancelled by the host', message: cancelReason }
        : prev,
    )
  }, [cancelReason])

  // Rescan goes to list_ports — a plain data tool with no UI binding, so
  // calling it refreshes this card without spawning another one.
  const rescan = useCallback(async (): Promise<void> => {
    setSelectedPath(null)
    setLoad({ kind: 'waiting' })
    try {
      const result = await callTool('list_ports')
      if (result.isError) {
        fail('list_ports failed', toolResultText(result) || 'list_ports returned an error')
        return
      }
      acceptCandidates(parseListPorts(result.structuredContent), meta.device)
    } catch (error) {
      fail('Could not rescan serial ports', errorMessage(error))
    }
  }, [callTool, acceptCandidates, fail, meta.device])

  // The choice is pushed to the agent's context; the agent opens the port.
  const sendSelection = useCallback(
    async (port: EnrichedPort): Promise<void> => {
      setSend({ kind: 'sending', path: port.path })
      try {
        await updateModelContext({
          content: [{ type: 'text', text: selectionText(port, meta.device) }],
          structuredContent: portSelection(port, meta.reason),
        })
        setSend({ kind: 'sent', path: port.path })
      } catch (error) {
        setSend({ kind: 'failed', path: port.path, message: errorMessage(error) })
      }
    },
    [updateModelContext, meta.device, meta.reason],
  )

  const candidates = load.kind === 'ready' ? load.candidates : NO_PORTS
  const aliases = aliasesByCanonical(candidates)
  const rows = visiblePorts(candidates, showAliases)
  const aliasCount = candidates.length - visiblePorts(candidates, false).length
  const selected = findPort(candidates, selectedPath)

  const toggleAliases = useCallback((): void => {
    const next = !showAliases
    setShowAliases(next)
    // Folding must not leave the selection on a row nobody can see.
    if (!next && selected?.alias_of !== undefined) setSelectedPath(selected.alias_of)
  }, [showAliases, selected])

  const selectPort = useCallback((path: string): void => {
    setSelectedPath(path)
  }, [])

  const retrySend = useCallback((): void => {
    if (send.kind !== 'failed') return
    const port = findPort(candidates, send.path)
    if (port === undefined) {
      setSend({
        kind: 'failed',
        path: send.path,
        message: `${send.path} is no longer among the candidates — pick another port.`,
      })
      return
    }
    void sendSelection(port)
  }, [send, candidates, sendSelection])

  if (widget.connectError) {
    return (
      <div className="pp-shell">
        <Card title="Select a serial port" head={<CardSpacer />}>
          <ObError
            title="Host connection failed"
            detail={`${widget.connectError.message} — run inside the harness (pnpm harness) or an MCP Apps host.`}
          />
        </Card>
      </div>
    )
  }

  const canSend =
    selected !== undefined &&
    !isBlocked(selected) &&
    widget.isConnected &&
    send.kind !== 'sending' &&
    !(send.kind === 'sent' && send.path === selected.path)

  const head = (
    <>
      {meta.device !== undefined && <Chip>for {meta.device}</Chip>}
      <CardSpacer />
      {load.kind === 'waiting' && <span className="ob-loading" style={{ width: 74 }} />}
      {aliasCount > 0 && (
        <Btn variant="ghost" onClick={toggleAliases}>
          {aliasLabel(aliasCount, showAliases)}
        </Btn>
      )}
    </>
  )

  const body = (() => {
    if (load.kind === 'waiting') {
      return (
        <div className="ob-card__body">
          <ObLoading widths={['82%', '63%', '46%']} />
        </div>
      )
    }
    if (load.kind === 'failed') {
      return (
        <div className="ob-card__body">
          {/* No retry button here: the footer's RescanFoot is the single
              Rescan control, matching the empty state. */}
          <ObError
            title={load.title}
            detail={
              <>
                <span>{load.message}</span>
                <div className="pp-error-detail">
                  No candidates are shown. Rescan to enumerate the ports again.
                </div>
              </>
            }
          />
        </div>
      )
    }
    if (rows.length === 0) {
      return <ObEmpty>No serial ports detected. Plug in a device, then rescan.</ObEmpty>
    }
    return (
      <>
        {meta.reason !== undefined && (
          <div className="pp-ask">
            <span className="pp-ask__label">Asked because</span>
            <span>{meta.reason}</span>
          </div>
        )}
        <PortTable
          ports={rows}
          showAliases={showAliases}
          selectedPath={selectedPath}
          sentPath={send.kind === 'sent' ? send.path : null}
          onSelect={selectPort}
        />
        {send.kind === 'failed' && (
          <div className="pp-error-wrap">
            <ObError
              title="Could not send the selection"
              detail={
                <>
                  <span>{send.message}</span>
                  <div className="pp-error-detail">
                    Nothing reached the agent. Retry, or pick another port.
                  </div>
                </>
              }
              onRetry={retrySend}
            />
          </div>
        )}
      </>
    )
  })()

  const foot = (() => {
    if (load.kind === 'waiting') return <LoadingFoot />
    if (load.kind === 'failed') return <RescanFoot onRescan={() => void rescan()} />
    if (rows.length === 0) return <RescanFoot count={0} onRescan={() => void rescan()} />
    return (
      <PickerFoot
        count={rows.length}
        send={send}
        canSend={canSend}
        onSend={() => {
          if (selected !== undefined) void sendSelection(selected)
        }}
        onRescan={() => void rescan()}
      />
    )
  })()

  return (
    <div className="pp-shell">
      <Card title="Select a serial port" head={head} foot={foot} pad={false}>
        {body}
      </Card>
    </div>
  )
}
