import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type uPlot from 'uplot'
import { BadgeReplay } from '../../components/Badges'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { ObError } from '../../components/ObError'
import { ObTable, ObTableRow } from '../../components/ObTable'
import { Receipt } from '../../components/Receipt'
import type { WidgetHandle } from '../../mcp/useWidget'
import { densityBucketAt, drawDensityTracks, timeToPx } from '../../render/overlays'
import { anchorInHost, createTooltipHost, readUplotCursor } from '../../render/tooltip'
import {
  attachWheelPan,
  createUplotHost,
  cssVarReader,
  tokenColor,
  type CssVarReader,
  type UplotHost,
} from '../../render/uplot-host'
import type { CaptureData, CaptureFrame } from './capture'
import type { ResultRef } from './state'

const CAPTURE_HEIGHT = 150
const MONO_FONT = '10px ui-monospace, "SF Mono", Menlo, monospace'

type SendState =
  | { readonly kind: 'idle' }
  | { readonly kind: 'sending' }
  | { readonly kind: 'sent'; readonly seq: number }
  | { readonly kind: 'failed'; readonly message: string }

function hex2(byte: number): string {
  return byte.toString(16).toUpperCase().padStart(2, '0')
}

function relMs(tsMs: number, startedMs: number): string {
  return `+${(tsMs - startedMs).toLocaleString('en-US')}`
}

function buildOptions(width: number, startedMs: number, read: CssVarReader): uPlot.Options {
  return {
    width,
    height: CAPTURE_HEIGHT,
    legend: { show: false },
    padding: [8, 12, 0, 8],
    cursor: { y: false, points: { show: false }, drag: { x: true, y: false } },
    scales: { x: { time: false }, y: { range: [0, 1] } },
    axes: [
      {
        stroke: tokenColor(read, '--scope-muted'),
        font: MONO_FONT,
        size: 30,
        gap: 6,
        space: 80,
        // Relative time against the capture header's started_ms.
        values: (_u, splits) => splits.map((ms) => `+${((ms - startedMs) / 1000).toFixed(1)}s`),
        grid: { show: true, stroke: tokenColor(read, '--scope-grid'), width: 1 },
        ticks: { show: true, stroke: tokenColor(read, '--scope-faint'), width: 1, size: 5 },
      },
      { show: false },
    ],
    series: [
      {},
      { label: 'TX', stroke: tokenColor(read, '--s2'), paths: () => null, points: { show: false } },
      { label: 'RX', stroke: tokenColor(read, '--s1'), paths: () => null, points: { show: false } },
    ],
  }
}

export interface CaptureViewProps {
  readonly widget: WidgetHandle
  readonly capture: CaptureData
  readonly resultRef: ResultRef
}

/**
 * Capture workbench v1 per the W3 design card, saved-window edition: uPlot
 * owns the time axis (drag-select zoom, double-click reset, wheel pan), the
 * density overlay folds the delivered frames into TX/RX byte buckets, the
 * table lists the frames and the hex pane shows every byte of the selected
 * one. Selecting a frame pushes {kind:"frame_selection"} to the agent. The
 * window itself is the agent's choice — this card never calls capture_frames,
 * and the footer states how much of the window was actually delivered.
 */
export function CaptureView({ widget, capture, resultRef }: CaptureViewProps) {
  const mountRef = useRef<HTMLDivElement | null>(null)
  const hostRef = useRef<UplotHost | null>(null)
  const [selected, setSelected] = useState<number | null>(capture.frames.length > 0 ? 0 : null)
  const selectedRef = useRef<number | null>(selected)
  const [send, setSend] = useState<SendState>({ kind: 'idle' })

  const { header, frames } = capture
  const frame = selected === null ? undefined : frames[selected]

  const peakBytes = useMemo(
    () => capture.density.reduce((acc, b) => Math.max(acc, b.txBytes, b.rxBytes), 0),
    [capture],
  )

  useEffect(() => {
    const mount = mountRef.current
    if (!mount) return
    const { fromMs, toMs, bucketMs } = capture
    if (fromMs === undefined || toMs === undefined || bucketMs === undefined) return
    const read = cssVarReader(mount)
    const onDraw = (u: uPlot): void => {
      const x = u.scales.x
      if (x?.min == null || x.max == null) return
      const rect = { left: u.bbox.left, top: u.bbox.top, width: u.bbox.width, height: u.bbox.height }
      const p = window.devicePixelRatio || 1
      const scales = { fromMs: x.min, toMs: x.max, bucketMs, pxRatio: p }
      drawDensityTracks(u.ctx, rect, scales, capture.density, read)
      // Selected-frame marker: a dashed accent vertical at the frame's ts.
      const index = selectedRef.current
      const marked = index === null ? undefined : frames[index]
      if (marked !== undefined) {
        const px = timeToPx(rect, scales, marked.tsMs)
        if (px >= rect.left && px <= rect.left + rect.width) {
          u.ctx.save()
          u.ctx.strokeStyle = read('--brand')
          u.ctx.lineWidth = 1 * p
          u.ctx.setLineDash([4 * p, 3 * p])
          u.ctx.beginPath()
          u.ctx.moveTo(px, rect.top)
          u.ctx.lineTo(px, rect.top + rect.height)
          u.ctx.stroke()
          u.ctx.setLineDash([])
          u.ctx.fillStyle = read('--brand')
          u.ctx.fillRect(px - 3 * p, rect.top, 6 * p, 2 * p)
          u.ctx.restore()
        }
      }
    }
    // Hover tooltip on the density band, fed by uPlot's setCursor hook.
    // Density is sparse: a time with no bucket has no data and shows nothing.
    const tooltip = createTooltipHost(mount)
    const onCursor = (u: uPlot): void => {
      // A pressed button means a drag-zoom is in progress; the selection box
      // is the feedback for that gesture, not a tooltip.
      if ((u.cursor.event?.buttons ?? 0) !== 0) {
        tooltip.hide()
        return
      }
      const cursor = readUplotCursor(u, window.devicePixelRatio || 1)
      if (cursor === null) {
        tooltip.hide()
        return
      }
      const bucket = densityBucketAt(capture.density, bucketMs, u.posToVal(cursor.cssX, 'x'))
      if (bucket === null) {
        tooltip.hide()
        return
      }
      const anchor = anchorInHost(u, mount, cursor.cssX, cursor.cssY)
      tooltip.show(anchor.x, anchor.y, [
        { label: 't', value: `${relMs(bucket.t0, header.startedMs)} ms` },
        { label: 'TX', value: `${bucket.txBytes} B` },
        { label: 'RX', value: `${bucket.rxBytes} B` },
      ])
    }
    const opts = buildOptions(Math.max(320, mount.clientWidth), header.startedMs, read)
    const host = createUplotHost({
      target: mount,
      opts: { ...opts, hooks: { ...opts.hooks, setCursor: [onCursor] } },
      data: [[fromMs, toMs], [null, null], [null, null]],
      onDraw,
    })
    hostRef.current = host
    const detachPan = attachWheelPan(host.u, fromMs, toMs)
    const observer = new ResizeObserver(() => {
      const width = mount.clientWidth
      if (width > 0) host.setSize(width, CAPTURE_HEIGHT)
    })
    observer.observe(mount)
    return () => {
      observer.disconnect()
      detachPan()
      tooltip.destroy()
      host.destroy()
      hostRef.current = null
    }
  }, [capture, frames, header.startedMs])

  // Selection and host theme switches repaint through the same path: the
  // overlay reads fresh token values on every redraw.
  useEffect(() => {
    selectedRef.current = selected
    hostRef.current?.redraw()
  }, [selected])
  const theme = widget.theme
  useEffect(() => {
    hostRef.current?.redraw()
  }, [theme])

  const pushSelection = useCallback(
    (index: number): void => {
      const chosen = frames[index]
      if (chosen === undefined) return
      setSend({ kind: 'sending' })
      widget
        .updateModelContext({
          content: [
            {
              type: 'text',
              text: `User selected frame seq ${chosen.seq} (${chosen.dir.toUpperCase()}, ${chosen.len} B) at t${relMs(chosen.tsMs, header.startedMs)} ms in the capture window.`,
            },
          ],
          structuredContent: {
            kind: 'frame_selection',
            seq: chosen.seq,
            ts_ms: chosen.tsMs,
            dir: chosen.dir,
          },
        })
        .then(() => setSend({ kind: 'sent', seq: chosen.seq }))
        .catch((error: unknown) => {
          const detail = error instanceof Error ? error.message : String(error)
          setSend({ kind: 'failed', message: detail })
        })
    },
    [widget, frames, header.startedMs],
  )

  const onSelect = useCallback(
    (index: number): void => {
      setSelected(index)
      pushSelection(index)
    },
    [pushSelection],
  )

  const windowNote = `${frames.length} of ${capture.totalInWindow} in window · window chosen by the agent`
  const counts = [
    windowNote,
    ...(capture.truncatedFrames > 0 ? [`+${capture.truncatedFrames} truncated`] : []),
    ...(capture.bucketMs !== undefined
      ? [`buckets of ${capture.bucketMs} ms`, `peak ${peakBytes} B/bucket`]
      : []),
  ]

  return (
    <div className="viewer-root">
      <Card
        title="Capture workbench"
        head={
          <>
            {capture.replay && <BadgeReplay />}
            {header.path !== undefined && <Chip>{header.path}</Chip>}
            <CardSpacer />
            <span className="live-tag">SAVED WINDOW</span>
          </>
        }
        foot={
          <>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
              {resultRef.path ?? (resultRef.source === 'file' ? resultRef.uri : 'inline result')}
            </span>
            <CardSpacer />
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>{windowNote}</span>
          </>
        }
      >
        {frames.length === 0 ? (
          <div className="ob-empty">
            no frames delivered — the window held {capture.totalInWindow} and the agent delivered
            none of them
          </div>
        ) : (
          <>
            <div className="ob-scope ob-scope--capture">
              <div className="ob-scope__meta ob-scope__meta--tl">
                <span>
                  <b>{header.port}</b>
                </span>
              </div>
              <div className="ob-scope__meta ob-scope__meta--tr tl-legend">
                <span>
                  <i style={{ background: 'var(--s2)' }} />
                  TX
                </span>
                <span>
                  <i style={{ background: 'var(--s1)' }} />
                  RX
                </span>
              </div>
              <div ref={mountRef} className="cap-plot" />
              {capture.replay && (
                <div className="ob-watermark">
                  <span>REPLAY</span>
                </div>
              )}
            </div>
            <div className="viewer-kpi">
              {counts.map((text) => (
                <span key={text}>{text}</span>
              ))}
              <span className="tl-note">drag: zoom · double-click: reset · scroll: pan</span>
            </div>

            <div className="cap-table-wrap">
              <ObTable head={['seq', 't+ms', 'dir', 'len']} aria-label="Captured frames">
                {frames.map((entry: CaptureFrame, index) => (
                  <ObTableRow
                    key={index}
                    selectable
                    selected={selected === index}
                    onSelect={() => onSelect(index)}
                  >
                    <td className="mono">{entry.seq}</td>
                    <td className="mono">{relMs(entry.tsMs, header.startedMs)}</td>
                    <td>
                      <span className={entry.dir === 'tx' ? 'dir dir--tx' : 'dir dir--rx'}>
                        {entry.dir === 'tx' ? '▲ TX' : '▼ RX'}
                      </span>
                    </td>
                    <td className="mono">{entry.len}</td>
                  </ObTableRow>
                ))}
              </ObTable>
            </div>

            {frame !== undefined && (
              <>
                <p className="cap-hex-label">
                  {frame.dir.toUpperCase()} · seq {frame.seq} · {frame.len} B · t
                  {relMs(frame.tsMs, header.startedMs)} ms
                </p>
                {/* Raw bytes only: a capture window carries no parse
                    declaration, so no field is highlighted here. */}
                <div className="ob-hex">{frame.bytes.map(hex2).join(' ')}</div>
              </>
            )}

            <div className="diag-actions">
              {send.kind === 'sent' && (
                <Receipt>frame seq {send.seq} accepted by the host</Receipt>
              )}
              <span className="tl-note">row click: select frame + send to agent</span>
            </div>
            {send.kind === 'failed' && (
              <div style={{ marginTop: 10 }}>
                <ObError
                  title="Frame selection not sent"
                  detail={`${send.message} — nothing reached the agent.`}
                  onRetry={selected === null ? undefined : () => pushSelection(selected)}
                />
              </div>
            )}
          </>
        )}
      </Card>
    </div>
  )
}
