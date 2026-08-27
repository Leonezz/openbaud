import { useCallback, useEffect, useState } from 'react'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { ObEmpty } from '../../components/ObEmpty'
import { ObError } from '../../components/ObError'
import { ObLoading } from '../../components/ObLoading'
import { useWidget, type ToolResult } from '../../mcp/useWidget'
import { FailedFoot, IdleFoot, LoadingFoot, OpenedFoot, OpeningFoot, RescanFoot } from './Foot'
import { PortTable } from './PortTable'
import {
  MOCK_ECHO_CANDIDATE,
  parseListPorts,
  parseOpenResult,
  toolResultText,
  type OpenedSession,
  type PortCandidate,
} from './ports'

type ScanState =
  | { readonly kind: 'waiting' }
  | { readonly kind: 'failed'; readonly message: string }
  | { readonly kind: 'ready'; readonly ports: readonly PortCandidate[] }

type OpenState =
  | { readonly kind: 'idle' }
  | { readonly kind: 'opening'; readonly port: string }
  | { readonly kind: 'failed'; readonly port: string; readonly message: string }
  | { readonly kind: 'opened'; readonly session: OpenedSession }

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function PortPickerApp() {
  const widget = useWidget({ name: 'openbaud-port-picker', version: '0.1.0' })
  const { callTool, toolResult, cancelReason } = widget

  const [scan, setScan] = useState<ScanState>({ kind: 'waiting' })
  const [open, setOpen] = useState<OpenState>({ kind: 'idle' })
  const [selectedPath, setSelectedPath] = useState<string | null>(null)

  const applyScanResult = useCallback((result: ToolResult): void => {
    setOpen({ kind: 'idle' })
    if (result.isError) {
      setSelectedPath(null)
      setScan({ kind: 'failed', message: toolResultText(result) || 'list_ports returned an error' })
      return
    }
    try {
      const ports = parseListPorts(result.structuredContent)
      // Preselect the first candidate (single-candidate case included);
      // opening stays a human-confirmed action.
      setSelectedPath(ports[0]?.path ?? null)
      setScan({ kind: 'ready', ports })
    } catch (error) {
      setSelectedPath(null)
      setScan({ kind: 'failed', message: errorMessage(error) })
    }
  }, [])

  useEffect(() => {
    if (toolResult !== undefined) applyScanResult(toolResult)
  }, [toolResult, applyScanResult])

  useEffect(() => {
    if (cancelReason === undefined) return
    setScan((prev) =>
      prev.kind === 'waiting'
        ? { kind: 'failed', message: `Enumeration cancelled by host: ${cancelReason}` }
        : prev,
    )
  }, [cancelReason])

  const rescan = useCallback(async (): Promise<void> => {
    setSelectedPath(null)
    setOpen({ kind: 'idle' })
    setScan({ kind: 'waiting' })
    try {
      applyScanResult(await callTool('list_ports'))
    } catch (error) {
      setScan({ kind: 'failed', message: errorMessage(error) })
    }
  }, [callTool, applyScanResult])

  const openPort = useCallback(
    async (port: string): Promise<void> => {
      setOpen({ kind: 'opening', port })
      try {
        const result = await callTool('open', { port })
        if (result.isError) {
          setOpen({ kind: 'failed', port, message: toolResultText(result) || 'open returned an error' })
          return
        }
        setOpen({ kind: 'opened', session: parseOpenResult(result.structuredContent) })
      } catch (error) {
        setOpen({ kind: 'failed', port, message: errorMessage(error) })
      }
    },
    [callTool],
  )

  const useMockEcho = useCallback((): void => {
    setScan({ kind: 'ready', ports: [MOCK_ECHO_CANDIDATE] })
    setSelectedPath(MOCK_ECHO_CANDIDATE.path)
    setOpen({ kind: 'idle' })
  }, [])

  const selectPort = useCallback((path: string): void => {
    setSelectedPath(path)
    setOpen((prev) => (prev.kind === 'failed' ? { kind: 'idle' } : prev))
  }, [])

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

  const opened = open.kind === 'opened' ? open.session : null
  const selectable = open.kind === 'idle' || open.kind === 'failed'

  const head = (
    <>
      <CardSpacer />
      {scan.kind === 'waiting' && <span className="ob-loading" style={{ width: 74 }} />}
      {opened !== null && <Chip variant="accent">port open</Chip>}
    </>
  )

  const body = (() => {
    if (scan.kind === 'waiting') {
      return (
        <div className="ob-card__body">
          <ObLoading widths={['82%', '63%', '46%']} />
        </div>
      )
    }
    if (scan.kind === 'failed') {
      return (
        <div className="ob-card__body">
          <ObError title="Could not list serial ports" detail={<span>{scan.message}</span>} />
        </div>
      )
    }
    if (scan.ports.length === 0) {
      return <ObEmpty>No serial ports detected. Plug in a device or try mock:echo.</ObEmpty>
    }
    return (
      <>
        <PortTable
          ports={scan.ports}
          selectedPath={selectedPath}
          openedPath={opened?.port ?? null}
          selectable={selectable}
          onSelect={selectPort}
        />
        {open.kind === 'failed' && (
          <div className="pp-error-wrap">
            <ObError
              title="Open failed"
              detail={
                <>
                  <span>{open.message}</span>
                  <div className="pp-error-detail">No session was created. Pick another port or retry.</div>
                </>
              }
            />
          </div>
        )}
      </>
    )
  })()

  const foot = (() => {
    if (scan.kind === 'waiting') return <LoadingFoot />
    if (scan.kind === 'failed') {
      return <RescanFoot onUseMock={useMockEcho} onRescan={() => void rescan()} />
    }
    if (scan.ports.length === 0) {
      return <RescanFoot count={0} onUseMock={useMockEcho} onRescan={() => void rescan()} />
    }
    switch (open.kind) {
      case 'opening':
        return <OpeningFoot port={open.port} />
      case 'failed':
        return (
          <FailedFoot
            onReselect={() => setOpen({ kind: 'idle' })}
            onRetry={() => void openPort(open.port)}
          />
        )
      case 'opened':
        return <OpenedFoot session={open.session} />
      case 'idle':
        return (
          <IdleFoot
            count={scan.ports.length}
            canOpen={selectedPath !== null && widget.isConnected}
            onOpen={() => {
              if (selectedPath !== null) void openPort(selectedPath)
            }}
          />
        )
    }
  })()

  return (
    <div className="pp-shell">
      <Card title="Select a serial port" head={head} foot={foot} pad={false}>
        {body}
      </Card>
    </div>
  )
}
