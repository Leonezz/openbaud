// Live rolling waveform (W7 scope): the widget opens its own per-consumer
// stream_poll subscription (useStreamPoll) and plots parsed[field] for each
// declared y series over a ~512-point window. uPlot owns the axes; the A/B
// measurement cursors are painted by the shared overlay layer. Frames the
// server could not parse are counted (parse errors, field misses) and shown —
// the stream itself never stops for one bad frame.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type uPlot from 'uplot'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { Led } from '../../components/Led'
import type { WidgetHandle } from '../../mcp/useWidget'
import { drawDualCursors } from '../../render/overlays'
import {
  createUplotHost,
  cssVarReader,
  tokenColor,
  type CssVarReader,
  type UplotHost,
} from '../../render/uplot-host'
import type { ScopeSpec, StreamFrame, StreamRef } from './stream'
import { StreamErrorNotice, StreamStatusLine } from './StreamStatus'
import { POLL_INTERVAL_MS, useStreamPoll, type FrameDelivery } from './useStreamPoll'

const SCOPE_HEIGHT = 260
const WINDOW_POINTS = 512
const MONO_FONT = '10px ui-monospace, "SF Mono", Menlo, monospace'
/** Series color tokens in declaration order (widget.css --s1..--s4). */
const SERIES_TOKENS = ['--s1', '--s2', '--s3', '--s4'] as const

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

/** Latest per-series values for the readout row (never render-only data). */
interface Latest {
  readonly tsMs: number
  readonly values: readonly number[]
}

/** A parsed frame whose declared field is missing/non-numeric, counted. */
interface FieldMisses {
  readonly count: number
  readonly reason: string | undefined
}

function seconds(ms: number): string {
  return `${(ms / 1000).toFixed(1)} s`
}

function buildOptions(width: number, fields: readonly string[], read: CssVarReader): uPlot.Options {
  return {
    width,
    height: SCOPE_HEIGHT,
    legend: { show: false },
    padding: [26, 12, 0, 4],
    cursor: { points: { show: false }, drag: { x: false, y: false } },
    scales: { x: { time: false } },
    axes: [
      {
        stroke: tokenColor(read, '--scope-muted'),
        font: MONO_FONT,
        size: 30,
        gap: 6,
        space: 70,
        values: (_u, splits) => splits.map((ms) => seconds(ms)),
        grid: { show: true, stroke: tokenColor(read, '--scope-grid'), width: 1 },
        ticks: { show: true, stroke: tokenColor(read, '--scope-faint'), width: 1, size: 5 },
      },
      {
        stroke: tokenColor(read, '--scope-muted'),
        font: MONO_FONT,
        size: 44,
        gap: 5,
        grid: { show: true, stroke: tokenColor(read, '--scope-grid'), width: 1 },
        ticks: { show: false },
      },
    ],
    series: [
      {},
      ...fields.map((field, index) => ({
        label: field,
        stroke: tokenColor(read, SERIES_TOKENS[index] ?? '--s4'),
        width: 2,
        points: { show: false },
      })),
    ],
  }
}

export interface ScopeViewProps {
  readonly widget: WidgetHandle
  readonly stream: StreamRef
  readonly spec: ScopeSpec
}

export function ScopeView({ widget, stream, spec }: ScopeViewProps) {
  const fields = spec.y
  const mountRef = useRef<HTMLDivElement | null>(null)
  const hostRef = useRef<UplotHost | null>(null)
  // Rolling buffer: x (ts_ms relative to the first frame) + one y per field.
  const bufRef = useRef<{ base: number | undefined; xs: number[]; ys: number[][] }>({
    base: undefined,
    xs: [],
    ys: fields.map(() => []),
  })
  const [cursors, setCursors] = useState<Cursors>({ a: null, b: null })
  const cursorsRef = useRef<Cursors>(cursors)
  const [latest, setLatest] = useState<Latest | undefined>(undefined)
  const [misses, setMisses] = useState<FieldMisses>({ count: 0, reason: undefined })

  const onFrames = useCallback(
    (frames: readonly StreamFrame[], delivery: FrameDelivery): void => {
      const buf = bufRef.current
      if (delivery.reset) {
        // New subscription = new seq/ts base; mixing it into the old trace
        // would draw one line across two clocks.
        buf.base = undefined
        buf.xs.length = 0
        for (const series of buf.ys) series.length = 0
        setCursors({ a: null, b: null })
      }
      let missed = 0
      let missReason: string | undefined
      for (const frame of frames) {
        if (frame.parsed === undefined) continue // parse_error — counted by the hook
        const values: number[] = []
        for (const field of fields) {
          const value = frame.parsed[field]
          if (typeof value !== 'number' || !Number.isFinite(value)) {
            missed += 1
            missReason ??= `parsed.${JSON.stringify(field)} is ${value === undefined ? 'missing' : 'not a finite number'} on frame seq ${frame.seq}`
            break
          }
          values.push(value)
        }
        if (values.length !== fields.length) continue
        buf.base ??= frame.tsMs
        buf.xs.push(frame.tsMs - buf.base)
        for (const [index, value] of values.entries()) buf.ys[index]?.push(value)
        setLatest({ tsMs: frame.tsMs - buf.base, values })
      }
      const overflow = buf.xs.length - WINDOW_POINTS
      if (overflow > 0) {
        buf.xs.splice(0, overflow)
        for (const series of buf.ys) series.splice(0, overflow)
      }
      if (missed > 0) {
        setMisses((prev) => ({ count: prev.count + missed, reason: missReason ?? prev.reason }))
      }
      hostRef.current?.u.setData([buf.xs, ...buf.ys] as uPlot.AlignedData)
    },
    [fields],
  )

  const status = useStreamPoll(widget, stream, onFrames)

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
        bucketMs: 1,
        pxRatio: window.devicePixelRatio || 1,
      }
      drawDualCursors(u.ctx, rect, scales, cursorsRef.current.a, cursorsRef.current.b, read)
    }
    const buf = bufRef.current
    const host = createUplotHost({
      target: mount,
      opts: buildOptions(Math.max(320, mount.clientWidth), fields, read),
      data: [buf.xs, ...buf.ys] as uPlot.AlignedData,
      onDraw,
    })
    hostRef.current = host

    // Click sets A then B (drag is disabled — the window scrolls on its own).
    const over = host.u.over
    let downX = 0
    const onDown = (event: MouseEvent): void => {
      downX = event.clientX
    }
    const onClick = (event: MouseEvent): void => {
      if (Math.abs(event.clientX - downX) > 3 || event.detail > 1) return
      const box = over.getBoundingClientRect()
      const ms = host.u.posToVal(event.clientX - box.left, 'x')
      if (!Number.isFinite(ms)) return
      setCursors((prev) => nextCursors(prev, ms))
    }
    over.addEventListener('mousedown', onDown)
    over.addEventListener('click', onClick)

    const observer = new ResizeObserver(() => {
      const width = mount.clientWidth
      if (width > 0) host.setSize(width, SCOPE_HEIGHT)
    })
    observer.observe(mount)
    return () => {
      observer.disconnect()
      over.removeEventListener('mousedown', onDown)
      over.removeEventListener('click', onClick)
      host.destroy()
      hostRef.current = null
    }
  }, [fields])

  useEffect(() => {
    cursorsRef.current = cursors
    hostRef.current?.redraw()
  }, [cursors])
  const theme = widget.theme
  useEffect(() => {
    hostRef.current?.redraw()
  }, [theme])

  const { a, b } = cursors
  const gapMs = a !== null && b !== null ? Math.round(Math.abs(b - a)) : null
  const lossy = status.droppedFrames > 0 || status.droppedChunks > 0
  const unitOf = useMemo(
    () =>
      (field: string): string =>
        status.units[field] !== undefined ? ` ${status.units[field]}` : '',
    [status.units],
  )

  return (
    <div className="viewer-root">
      <Card
        title="Live scope"
        head={
          <>
            <Chip>
              {stream.parse.device} · {stream.parse.command}
            </Chip>
            <Chip>session {stream.sessionId}</Chip>
            <CardSpacer />
            <span className="live-tag">STREAM · PER-CONSUMER SUBSCRIPTION</span>
          </>
        }
      >
        <div className="ob-scope ob-scope--live">
          <div className="ob-scope__meta ob-scope__meta--tl">
            <span>
              window <b>{WINDOW_POINTS} pts</b> · {fields.length} series
            </span>
          </div>
          {lossy && (
            <div className="ob-scope__meta ob-scope__meta--tr stream-loss">
              <Led tone="err" />
              <span>
                drop <b>{status.droppedFrames}f/{status.droppedChunks}c</b>
              </span>
            </div>
          )}
          <div className="ob-scope__meta ob-scope__meta--br">
            <span>
              poll <b>{POLL_INTERVAL_MS} ms</b>
            </span>
          </div>
          <div ref={mountRef} className="scope-plot" />
        </div>
        <div className="viewer-kpi">
          {fields.map((field, index) => (
            <span key={field} className="scope-series">
              <i style={{ background: `var(${SERIES_TOKENS[index] ?? '--s4'})` }} />
              {field}{' '}
              <b>
                {latest !== undefined ? `${latest.values[index]}${unitOf(field)}` : 'no data yet'}
              </b>
            </span>
          ))}
          <span>
            t <b>{latest !== undefined ? seconds(latest.tsMs) : '—'}</b>
          </span>
        </div>
        <div className="viewer-kpi">
          <span>
            A <b>{a !== null ? seconds(a) : 'not set'}</b>
          </span>
          <span>
            B <b>{b !== null ? seconds(b) : 'not set'}</b>
          </span>
          <span>
            gap <b>{gapMs !== null ? `${gapMs} ms` : 'set both cursors'}</b>
          </span>
          <span className="tl-note">click: set A/B</span>
        </div>
        <StreamStatusLine status={status} />
        {misses.count > 0 && (
          <div className="viewer-kpi">
            <span className="stream-warn">
              {misses.count} parsed {misses.count === 1 ? 'frame misses' : 'frames miss'} a declared
              y field{misses.reason !== undefined ? ` — first: ${misses.reason}` : ''}
            </span>
          </div>
        )}
        <StreamErrorNotice status={status} />
      </Card>
    </div>
  )
}
