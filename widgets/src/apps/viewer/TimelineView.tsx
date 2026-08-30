import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type uPlot from 'uplot'
import { BadgeReplay } from '../../components/Badges'
import { Btn } from '../../components/Btn'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { ObError } from '../../components/ObError'
import { Receipt } from '../../components/Receipt'
import type { WidgetHandle } from '../../mcp/useWidget'
import {
  densityBucketAt,
  drawDensityTracks,
  drawDualCursors,
  drawEventMarkers,
  hitEventMarker,
  type EventMarker,
} from '../../render/overlays'
import {
  anchorInHost,
  createTooltipHost,
  readUplotCursor,
  type TooltipLine,
} from '../../render/tooltip'
import {
  attachWheelPan,
  createUplotHost,
  cssVarReader,
  tokenColor,
  type CssVarReader,
  type UplotHost,
} from '../../render/uplot-host'
import type { ResultRef } from './state'
import type { TimelineData, TimelineEvent } from './timeline'

const TIMELINE_HEIGHT = 236
const MONO_FONT = '10px ui-monospace, "SF Mono", Menlo, monospace'

interface Cursors {
  readonly a: number | null
  readonly b: number | null
}

/** First click sets A, second sets B, third starts over with a new A. */
function nextCursors(prev: Cursors, ms: number): Cursors {
  if (prev.a === null) return { a: ms, b: prev.b }
  if (prev.b === null) return { a: prev.a, b: ms }
  return { a: ms, b: null }
}

type SendState =
  | { readonly kind: 'idle' }
  | { readonly kind: 'sending' }
  | { readonly kind: 'sent'; readonly gapMs: number }
  | { readonly kind: 'failed'; readonly message: string }

/** Wall-clock label (local time) for an epoch-ms timestamp. */
function clock(ms: number, withMillis = false): string {
  const date = new Date(ms)
  const base = [date.getHours(), date.getMinutes(), date.getSeconds()]
    .map((part) => String(part).padStart(2, '0'))
    .join(':')
  return withMillis ? `${base}.${String(date.getMilliseconds()).padStart(3, '0')}` : base
}

/**
 * Tooltip rows for one audit event — only keys the event object really
 * carries; an absent command/workflow/outcome contributes no row at all.
 */
function eventTooltip(event: TimelineEvent): readonly TooltipLine[] {
  return [
    { label: 't', value: clock(event.tsMs, true) },
    { label: 'kind', value: event.kind },
    { label: 'tool', value: event.tool },
    ...(event.command !== undefined ? [{ label: 'command', value: event.command }] : []),
    ...(event.workflow !== undefined ? [{ label: 'workflow', value: event.workflow }] : []),
    ...(event.outcome !== undefined ? [{ label: 'outcome', value: event.outcome }] : []),
    { label: 'status', value: event.ok ? 'ok' : 'fail' },
  ]
}

function toMarker(event: TimelineEvent): EventMarker {
  return {
    tsMs: event.tsMs,
    kind: event.kind,
    label: event.command ?? event.workflow ?? event.tool,
    ok: event.ok,
  }
}

function buildOptions(width: number, read: CssVarReader): uPlot.Options {
  return {
    width,
    height: TIMELINE_HEIGHT,
    legend: { show: false },
    // Top padding keeps the gap readout clear of the scope's corner meta row.
    padding: [22, 12, 0, 8],
    cursor: { y: false, points: { show: false }, drag: { x: true, y: false } },
    scales: {
      // Epoch-ms x kept numeric; wall-clock formatting happens in axis values.
      x: { time: false },
      // The y scale is owned by the overlay painters (normalized density).
      y: { range: [0, 1] },
    },
    axes: [
      {
        stroke: tokenColor(read, '--scope-muted'),
        font: MONO_FONT,
        size: 30,
        gap: 6,
        space: 90,
        values: (_u, splits) => splits.map((ms) => clock(ms)),
        grid: { show: true, stroke: tokenColor(read, '--scope-grid'), width: 1 },
        ticks: { show: true, stroke: tokenColor(read, '--scope-faint'), width: 1, size: 5 },
      },
      { show: false },
    ],
    // Series exist for the legend contract only; the density band itself is
    // painted by the overlay. Colors are functions per the uplot-host rule.
    series: [
      {},
      { label: 'TX', stroke: tokenColor(read, '--s2'), paths: () => null, points: { show: false } },
      { label: 'RX', stroke: tokenColor(read, '--s1'), paths: () => null, points: { show: false } },
    ],
  }
}

export interface TimelineViewProps {
  readonly widget: WidgetHandle
  readonly timeline: TimelineData
  readonly resultRef: ResultRef
}

/**
 * Session-timeline card per the W11 design card: uPlot owns the time axis and
 * its built-in drag-select zoom / double-click reset (wheel pan is added by
 * uplot-host); density tracks, event markers and the A/B cursors are painted
 * by the pure overlay layer on uPlot's draw hook. A saved slice of the audit
 * stream, drawn as-is — nothing here polls or animates, so reduced-motion has
 * no JS-driven motion to freeze.
 */
export function TimelineView({ widget, timeline, resultRef }: TimelineViewProps) {
  const mountRef = useRef<HTMLDivElement | null>(null)
  const hostRef = useRef<UplotHost | null>(null)
  const [cursors, setCursors] = useState<Cursors>({ a: null, b: null })
  const cursorsRef = useRef<Cursors>(cursors)
  const [send, setSend] = useState<SendState>({ kind: 'idle' })

  const markers = useMemo(() => timeline.events.map(toMarker), [timeline])
  const peakBytes = useMemo(
    () => timeline.density.reduce((acc, b) => Math.max(acc, b.txBytes, b.rxBytes), 0),
    [timeline],
  )

  useEffect(() => {
    const mount = mountRef.current
    if (!mount) return
    const read = cssVarReader(mount)
    const onDraw = (u: uPlot): void => {
      const x = u.scales.x
      if (x?.min == null || x.max == null) return
      const rect = { left: u.bbox.left, top: u.bbox.top, width: u.bbox.width, height: u.bbox.height }
      const scales = {
        fromMs: x.min,
        toMs: x.max,
        bucketMs: timeline.bucketMs,
        pxRatio: window.devicePixelRatio || 1,
      }
      drawDensityTracks(u.ctx, rect, scales, timeline.density, read)
      drawDualCursors(u.ctx, rect, scales, cursorsRef.current.a, cursorsRef.current.b, read)
      drawEventMarkers(u.ctx, rect, scales, markers, read)
    }
    // Hover tooltip: fed by uPlot's setCursor hook. An event marker outranks
    // the density bucket under it — one position, one tooltip.
    const tooltip = createTooltipHost(mount)
    const onCursor = (u: uPlot): void => {
      // A pressed button means a drag-zoom is in progress; the selection box
      // is the feedback for that gesture, not a tooltip.
      if ((u.cursor.event?.buttons ?? 0) !== 0) {
        tooltip.hide()
        return
      }
      const pxRatio = window.devicePixelRatio || 1
      const cursor = readUplotCursor(u, pxRatio)
      const x = u.scales.x
      if (cursor === null || x?.min == null || x.max == null) {
        tooltip.hide()
        return
      }
      const rect = { left: u.bbox.left, top: u.bbox.top, width: u.bbox.width, height: u.bbox.height }
      const scales = { fromMs: x.min, toMs: x.max, bucketMs: timeline.bucketMs, pxRatio }
      const anchor = anchorInHost(u, mount, cursor.cssX, cursor.cssY)
      const hit = hitEventMarker(rect, scales, markers, cursor.devX, cursor.devY)
      const event = hit === null ? undefined : timeline.events[hit]
      if (event !== undefined) {
        tooltip.show(anchor.x, anchor.y, eventTooltip(event))
        return
      }
      // Density is sparse: no bucket at this time means no data — no tooltip.
      const bucket = densityBucketAt(timeline.density, timeline.bucketMs, u.posToVal(cursor.cssX, 'x'))
      if (bucket === null) {
        tooltip.hide()
        return
      }
      tooltip.show(anchor.x, anchor.y, [
        { label: 't', value: clock(bucket.t0, true) },
        { label: 'TX', value: `${bucket.txBytes} B` },
        { label: 'RX', value: `${bucket.rxBytes} B` },
      ])
    }
    const opts = buildOptions(Math.max(320, mount.clientWidth), read)
    const host = createUplotHost({
      target: mount,
      opts: { ...opts, hooks: { ...opts.hooks, setCursor: [onCursor] } },
      data: [[timeline.fromMs, timeline.toMs], [null, null], [null, null]],
      onDraw,
    })
    hostRef.current = host
    const detachPan = attachWheelPan(host.u, timeline.fromMs, timeline.toMs)

    // Click (as opposed to a drag, which is the zoom gesture) sets A then B.
    const over = host.u.over
    let downX = 0
    const onDown = (event: MouseEvent): void => {
      downX = event.clientX
    }
    const onClick = (event: MouseEvent): void => {
      if (Math.abs(event.clientX - downX) > 3) return
      // The second click of a double-click (uPlot's zoom reset) must not turn
      // the gesture into a degenerate A=B pair at the click point.
      if (event.detail > 1) return
      const box = over.getBoundingClientRect()
      const ms = host.u.posToVal(event.clientX - box.left, 'x')
      if (!Number.isFinite(ms)) return
      setCursors((prev) => nextCursors(prev, ms))
    }
    over.addEventListener('mousedown', onDown)
    over.addEventListener('click', onClick)

    const observer = new ResizeObserver(() => {
      const width = mount.clientWidth
      if (width > 0) host.setSize(width, TIMELINE_HEIGHT)
    })
    observer.observe(mount)
    return () => {
      observer.disconnect()
      over.removeEventListener('mousedown', onDown)
      over.removeEventListener('click', onClick)
      detachPan()
      tooltip.destroy()
      host.destroy()
      hostRef.current = null
    }
  }, [timeline, markers])

  // Cursor moves and host theme switches repaint through the same path: the
  // color functions and the overlay read fresh values on every redraw.
  useEffect(() => {
    cursorsRef.current = cursors
    hostRef.current?.redraw()
  }, [cursors])
  const theme = widget.theme
  useEffect(() => {
    hostRef.current?.redraw()
  }, [theme])

  const { a, b } = cursors
  const window_ =
    a !== null && b !== null
      ? { fromMs: Math.round(Math.min(a, b)), toMs: Math.round(Math.max(a, b)) }
      : null
  const gapMs = window_ === null ? null : window_.toMs - window_.fromMs

  const sendQuietWindow = useCallback((): void => {
    if (window_ === null || gapMs === null) return
    setSend({ kind: 'sending' })
    widget
      .updateModelContext({
        content: [
          {
            type: 'text',
            text: `User marked a quiet window on the session timeline: ${clock(window_.fromMs, true)} to ${clock(window_.toMs, true)} wall clock (${gapMs} ms).`,
          },
        ],
        structuredContent: {
          kind: 'quiet_window',
          from_ms: window_.fromMs,
          to_ms: window_.toMs,
          gap_ms: gapMs,
        },
      })
      .then(() => setSend({ kind: 'sent', gapMs }))
      .catch((error: unknown) => {
        const detail = error instanceof Error ? error.message : String(error)
        setSend({ kind: 'failed', message: detail })
      })
  }, [widget, window_, gapMs])

  const counts = [
    `${timeline.events.length} events${timeline.truncatedEvents > 0 ? ` (+${timeline.truncatedEvents} truncated)` : ''}`,
    `${timeline.density.length} buckets of ${timeline.bucketMs} ms${timeline.truncatedBuckets > 0 ? ` (+${timeline.truncatedBuckets} truncated)` : ''}`,
    `peak ${peakBytes} B/bucket`,
  ]

  return (
    <div className="viewer-root">
      <Card
        title="Session timeline"
        head={
          <>
            {timeline.replay && <BadgeReplay />}
            <Chip>{timeline.sourcePath}</Chip>
            <CardSpacer />
            <span className="live-tag">WALL-CLOCK TIMESTAMPS</span>
          </>
        }
        foot={
          <>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
              {resultRef.path ?? (resultRef.source === 'file' ? resultRef.uri : 'inline result')}
            </span>
            <CardSpacer />
            {resultRef.bytes !== undefined && <Chip>{resultRef.bytes} B</Chip>}
          </>
        }
      >
        <div className="ob-scope ob-scope--timeline">
          <div className="ob-scope__meta ob-scope__meta--tl">
            <span>
              start <b>{clock(timeline.fromMs, true)}</b> · wall clock
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
          <div ref={mountRef} className="tl-plot" />
          {timeline.replay && (
            <div className="ob-watermark">
              <span>REPLAY</span>
            </div>
          )}
        </div>
        <div className="viewer-kpi">
          <span>
            A <b>{a !== null ? clock(a, true) : 'not set'}</b>
          </span>
          <span>
            B <b>{b !== null ? clock(b, true) : 'not set'}</b>
          </span>
          <span>
            gap <b>{gapMs !== null ? `${gapMs} ms` : 'set both cursors'}</b>
          </span>
        </div>
        <div className="viewer-kpi">
          {counts.map((text) => (
            <span key={text}>{text}</span>
          ))}
        </div>
        <div className="tl-actions">
          <Btn
            variant="primary"
            disabled={window_ === null || send.kind === 'sending'}
            onClick={sendQuietWindow}
          >
            {send.kind === 'sending' ? 'Sending quiet window' : 'Send quiet window to agent'}
          </Btn>
          {send.kind === 'sent' && <Receipt>quiet window ({send.gapMs} ms) sent to the agent</Receipt>}
          <span className="tl-note">click: set A/B · drag: zoom · double-click: reset · scroll: pan</span>
        </div>
        {send.kind === 'failed' && (
          <div style={{ marginTop: 10 }}>
            <ObError
              title="Quiet window not sent"
              detail={`${send.message} — nothing reached the agent.`}
              onRetry={sendQuietWindow}
            />
          </div>
        )}
      </Card>
    </div>
  )
}
