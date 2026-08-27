// Pure schema dispatch for the viewer app: decides how a structuredContent
// summary (crates/openbaud/src/run.rs execute_command shape, summarized per
// output.rs) should be rendered. No DOM, no React — unit-testable as-is.
import type { OpenbaudSummary, ToolArgs } from '../../mcp/useWidget'
import type { PolarPoint } from '../../render/polar'

export type JsonRecord = Record<string, unknown>

export function isRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * The server summarizer replaces the tail of long arrays with a single
 * `{"truncated": N}` marker (output.rs). Returns N for such a marker.
 */
export function truncationMarker(value: unknown): number | undefined {
  if (!isRecord(value)) return undefined
  const keys = Object.keys(value)
  if (keys.length === 1 && keys[0] === 'truncated' && typeof value.truncated === 'number') {
    return value.truncated
  }
  return undefined
}

export interface PolarScan {
  readonly points: readonly PolarPoint[]
  /** Elements the server dropped from the array (truncation marker sum). */
  readonly truncatedPoints: number
  /** False when any element lacks a numeric `intensity` (ramp is hidden). */
  readonly hasIntensity: boolean
  readonly simulatedScene: boolean
  /** "OBP/<version>" when the parsed frame carries the OBP magic. */
  readonly protocol: string | undefined
  readonly device: string | undefined
  readonly command: string | undefined
  readonly outcome: string | undefined
  readonly seq: number | undefined
  readonly uptimeMs: number | undefined
}

export type DispatchedView =
  | { readonly kind: 'polar'; readonly scan: PolarScan }
  | { readonly kind: 'generic' }

interface PointsHit {
  readonly points: readonly PolarPoint[]
  readonly truncated: number
  readonly hasIntensity: boolean
}

/** An array qualifies when every non-marker element has angle_deg + distance_mm. */
function asPolarArray(value: unknown): PointsHit | undefined {
  if (!Array.isArray(value)) return undefined
  const points: PolarPoint[] = []
  let truncated = 0
  let hasIntensity = true
  for (const element of value) {
    const marker = truncationMarker(element)
    if (marker !== undefined) {
      truncated += marker
      continue
    }
    if (!isRecord(element)) return undefined
    const angle = element.angle_deg
    const distance = element.distance_mm
    if (typeof angle !== 'number' || typeof distance !== 'number') return undefined
    const intensity = typeof element.intensity === 'number' ? element.intensity : undefined
    if (intensity === undefined) hasIntensity = false
    points.push({ angleDeg: angle, distanceMm: distance, intensity: intensity ?? 0 })
  }
  if (points.length === 0) return undefined
  return { points, truncated, hasIntensity }
}

const SEARCH_DEPTH = 3

function findPolarArray(value: JsonRecord, depth: number): PointsHit | undefined {
  for (const child of Object.values(value)) {
    const hit = asPolarArray(child)
    if (hit) return hit
  }
  if (depth >= SEARCH_DEPTH) return undefined
  for (const child of Object.values(value)) {
    if (!isRecord(child)) continue
    const hit = findPolarArray(child, depth + 1)
    if (hit) return hit
  }
  return undefined
}

function asString(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined
}

function asNumber(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined
}

// 0x4F42 = "OB", the OBP frame magic (crates/openbaud-core frame header).
const OBP_MAGIC = 0x4f42

function protocolLabel(parsed: JsonRecord): string | undefined {
  if (parsed.magic !== OBP_MAGIC) return undefined
  const version = asNumber(parsed.version)
  return version === undefined ? undefined : `OBP/${version}`
}

export function dispatchResult(structured: OpenbaudSummary): DispatchedView {
  const parsed = structured.parsed
  if (!isRecord(parsed)) return { kind: 'generic' }
  const hit = findPolarArray(parsed, 0)
  if (!hit) return { kind: 'generic' }
  return {
    kind: 'polar',
    scan: {
      points: hit.points,
      truncatedPoints: hit.truncated,
      hasIntensity: hit.hasIntensity,
      simulatedScene: asNumber(parsed.simulated_scene) === 1,
      protocol: protocolLabel(parsed),
      device: asString(structured.device),
      command: asString(structured.command),
      outcome: asString(structured.outcome),
      seq: asNumber(parsed.seq),
      uptimeMs: asNumber(parsed.uptime_ms),
    },
  }
}

/**
 * Tool name for re-polling. The host names the originating tool in
 * hostContext.toolInfo.tool; without it, arguments carrying string device +
 * command fields can only be openbaud's run_command.
 */
export function inferToolName(hostToolName: string | undefined, args: ToolArgs): string | undefined {
  if (hostToolName !== undefined) return hostToolName
  if (args !== undefined && typeof args.device === 'string' && typeof args.command === 'string') {
    return 'run_command'
  }
  return undefined
}

export interface RadarScale {
  readonly maxDistanceMm: number
  readonly ringMaxMm: number
  readonly ringStepMm: number
}

/**
 * Ring layout: the design baseline (500 mm steps, rings to 2500, canvas edge
 * at 2800) holds while every point fits; farther data doubles the step so the
 * geometry ratios (5 rings + 0.6-step margin) stay identical.
 */
export function radarScale(points: readonly PolarPoint[]): RadarScale {
  let farthest = 0
  for (const point of points) {
    if (point.distanceMm > farthest) farthest = point.distanceMm
  }
  let step = 500
  while (step * 5.6 < farthest) step *= 2
  return { ringStepMm: step, ringMaxMm: step * 5, maxDistanceMm: step * 5.6 }
}
