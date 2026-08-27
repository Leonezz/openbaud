// Viewer state machine (pure): every tool result — pushed by the host or
// pulled by the poll loop — goes through ingestResult. Failures never clear
// the last good radar frame; they surface as an explicit error banner.
import { openbaudStructured, type OpenbaudSummary, type ToolResult } from '../../mcp/useWidget'
import { dispatchResult, type PolarScan } from './dispatch'

export interface RadarState {
  /** Last good frame (outcome normal); stays on screen through failures. */
  readonly current: PolarScan | undefined
  /** The frame before it, drawn as the 30% afterglow. */
  readonly ghost: PolarScan | undefined
  readonly error: string | undefined
}

export const INITIAL_RADAR: RadarState = { current: undefined, ghost: undefined, error: undefined }

export function applyScan(state: RadarState, scan: PolarScan): RadarState {
  return { current: scan, ghost: state.current, error: undefined }
}

export function applyFailure(state: RadarState, message: string): RadarState {
  return { ...state, error: message }
}

export interface IntensityRange {
  readonly min: number
  readonly max: number
}

/** Union range over current+ghost — legend and dot colors must agree. */
export function intensityRange(state: RadarState): IntensityRange {
  const scans = [state.current, state.ghost].filter(
    (scan): scan is PolarScan => scan !== undefined && scan.hasIntensity,
  )
  if (scans.length === 0) {
    // No intensity data: a symmetric fake range parks every 0-intensity point
    // on the middle ramp bin (the legend is hidden in this case).
    return { min: -2, max: 2 }
  }
  let min = Infinity
  let max = -Infinity
  for (const scan of scans) {
    for (const point of scan.points) {
      if (point.intensity < min) min = point.intensity
      if (point.intensity > max) max = point.intensity
    }
  }
  return { min, max }
}

export type ViewerState =
  | { readonly kind: 'idle' }
  | { readonly kind: 'error'; readonly message: string }
  | { readonly kind: 'generic'; readonly structured: OpenbaudSummary }
  | { readonly kind: 'polar'; readonly radar: RadarState }

export const INITIAL_VIEWER: ViewerState = { kind: 'idle' }

export function errorText(result: ToolResult): string {
  const texts = result.content.flatMap((block) => (block.type === 'text' ? [block.text] : []))
  const joined = texts.join('\n').trim()
  return joined !== '' ? joined : 'tool call failed (host returned no error text)'
}

export function failWith(prev: ViewerState, message: string): ViewerState {
  if (prev.kind === 'polar') {
    return { kind: 'polar', radar: applyFailure(prev.radar, message) }
  }
  return { kind: 'error', message }
}

function describeNonScan(structured: OpenbaudSummary): string {
  const outcome = typeof structured.outcome === 'string' ? structured.outcome : undefined
  if (outcome !== undefined && outcome !== 'normal') {
    return `frame outcome ${outcome} — expected normal; keeping last good frame`
  }
  return 'result carried no radar points — keeping last good frame'
}

export function ingestResult(prev: ViewerState, result: ToolResult): ViewerState {
  if (result.isError) return failWith(prev, errorText(result))
  const structured = openbaudStructured(result)
  if (structured === undefined) {
    return failWith(prev, 'tool result carried no structuredContent — nothing to render')
  }
  const view = dispatchResult(structured)
  if (view.kind === 'polar') {
    const { scan } = view
    if (scan.outcome !== undefined && scan.outcome !== 'normal') {
      return failWith(prev, `frame outcome ${scan.outcome} — expected normal`)
    }
    const radar = prev.kind === 'polar' ? prev.radar : INITIAL_RADAR
    return { kind: 'polar', radar: applyScan(radar, scan) }
  }
  if (prev.kind === 'polar') {
    return { kind: 'polar', radar: applyFailure(prev.radar, describeNonScan(structured)) }
  }
  return { kind: 'generic', structured }
}
