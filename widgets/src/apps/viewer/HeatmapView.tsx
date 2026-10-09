// Live rows×cols heatmap: the same useStreamPoll state machine as ScopeView,
// rendering the newest frame's parsed[data] array through the shared heat
// grid (HeatGrid.tsx). A frame whose grid does not fit the declared rows×cols
// is counted and named — the last good grid stays on screen.
import { useCallback, useRef, useState } from 'react'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { Led } from '../../components/Led'
import type { WidgetHandle } from '../../mcp/useWidget'
import { HeatCanvas, HeatEmpty, HeatLegend } from './HeatGrid'
import type { HeatmapSpec, StreamFrame, StreamRef } from './stream'
import { StreamErrorNotice, StreamStatusLine } from './StreamStatus'
import { POLL_INTERVAL_MS, useStreamPoll, type FrameDelivery } from './useStreamPoll'

interface Grid {
  readonly values: readonly number[]
  readonly tsMs: number
}

/** Grid frames that did not fit the declared shape, counted and named. */
interface BadGrids {
  readonly count: number
  readonly reason: string | undefined
}

function seconds(ms: number): string {
  return `${(ms / 1000).toFixed(1)} s`
}

/** The newest well-formed grid in `frames`, or the reason none fit. */
function latestGrid(
  frames: readonly StreamFrame[],
  spec: HeatmapSpec,
  baseTs: number | undefined,
): { grid: Grid | undefined; bad: number; reason: string | undefined } {
  const wanted = spec.rows * spec.cols
  let grid: Grid | undefined
  let bad = 0
  let reason: string | undefined
  for (const frame of frames) {
    if (frame.parsed === undefined) continue // parse_error — counted by the hook
    const cells = frame.parsed[spec.data]
    if (!Array.isArray(cells)) {
      bad += 1
      reason ??= `parsed.${JSON.stringify(spec.data)} is not an array on frame seq ${frame.seq}`
      continue
    }
    if (cells.length !== wanted) {
      bad += 1
      reason ??= `parsed.${JSON.stringify(spec.data)} holds ${cells.length} values on frame seq ${frame.seq} — the heatmap declares ${spec.rows}×${spec.cols} = ${wanted}`
      continue
    }
    if (!cells.every((v): v is number => typeof v === 'number' && Number.isFinite(v))) {
      bad += 1
      reason ??= `parsed.${JSON.stringify(spec.data)} carries a non-numeric cell on frame seq ${frame.seq}`
      continue
    }
    grid = { values: cells, tsMs: frame.tsMs - (baseTs ?? frame.tsMs) }
  }
  return { grid, bad, reason }
}

export interface HeatmapViewProps {
  readonly widget: WidgetHandle
  readonly stream: StreamRef
  readonly spec: HeatmapSpec
}

export function HeatmapView({ widget, stream, spec }: HeatmapViewProps) {
  const baseTsRef = useRef<number | undefined>(undefined)
  const [grid, setGrid] = useState<Grid | undefined>(undefined)
  const [bad, setBad] = useState<BadGrids>({ count: 0, reason: undefined })

  const onFrames = useCallback(
    (frames: readonly StreamFrame[], delivery: FrameDelivery): void => {
      // New subscription = new ts base for the frame timestamp readout.
      if (delivery.reset) baseTsRef.current = undefined
      const firstParsed = frames.find((f) => f.parsed !== undefined)
      if (baseTsRef.current === undefined && firstParsed !== undefined) {
        baseTsRef.current = firstParsed.tsMs
      }
      const read = latestGrid(frames, spec, baseTsRef.current)
      if (read.grid !== undefined) setGrid(read.grid)
      if (read.bad > 0) {
        setBad((prev) => ({ count: prev.count + read.bad, reason: read.reason ?? prev.reason }))
      }
    },
    [spec],
  )

  const status = useStreamPoll(widget, stream, onFrames)

  const unit = status.units[spec.data]
  const lossy = status.droppedFrames > 0 || status.droppedChunks > 0

  return (
    <div className="viewer-root">
      <Card
        title="Live heatmap"
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
        <div className="ob-scope ob-scope--heatmap">
          <div className="ob-scope__meta ob-scope__meta--tl">
            <span>
              {spec.data} <b>{spec.rows}×{spec.cols}</b>
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
          <div className="ob-scope__meta ob-scope__meta--bl">
            <span>
              frame <b>{grid !== undefined ? `t ${seconds(grid.tsMs)}` : 'none yet'}</b>
            </span>
          </div>
          <div className="ob-scope__meta ob-scope__meta--br">
            <span>
              poll <b>{POLL_INTERVAL_MS} ms</b>
            </span>
          </div>
          {grid !== undefined ? (
            <HeatCanvas rows={spec.rows} cols={spec.cols} values={grid.values} theme={widget.theme} />
          ) : (
            <HeatEmpty text="waiting for the first grid frame" />
          )}
        </div>
        <HeatLegend values={grid?.values} unit={unit} />
        <StreamStatusLine status={status} />
        {bad.count > 0 && (
          <div className="viewer-kpi">
            <span className="stream-warn">
              {bad.count} {bad.count === 1 ? 'frame does' : 'frames do'} not fit the declared grid
              {bad.reason !== undefined ? ` — first: ${bad.reason}` : ''}
            </span>
          </div>
        )}
        <StreamErrorNotice status={status} />
      </Card>
    </div>
  )
}
