// Pure schema dispatch for the viewer app: decides how a full result object
// (crates/openbaud/src/run.rs execute_command shape) should be rendered.
//
// openbaud never guesses at field names. A result becomes a chart only when
// something declared how, the way a visualization grammar does it: one mark
// kind plus a mapping from visual channel to field. Two places may carry that
// declaration — the result's own top-level `view` (written by the profile
// author beside the parse spec) and show_result's `encoding` in
// structuredContent, which overrides `view` for the case where the agent
// decoded the bytes itself and the YAML has no parse/view at all. The field
// names inside belong to the device: a radar reporting `bearing`/`range_mm`
// renders exactly as well as one reporting `angle_deg`/`distance_mm`, and this
// module assumes neither.
//
// No declaration is normal — the result falls through to the key/value card. A
// declaration that does not fit the data is not: it surfaces as `kind:
// 'invalid'` with a reason naming the channel and the element, never a silent
// demotion that would read as "this result simply isn't a chart".
//
// The object arrives from the openbaud://result/… resource show_result points
// at, so arrays are complete; the truncation marker below still exists because
// a small inline result may have been summarized per output.rs before it was
// handed to show_result. No DOM, no React — unit-testable as-is.
import type { OpenbaudSummary } from '../../mcp/useWidget'
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

/** A `view` / `encoding` declaration, validated. Only "polar" exists so far. */
export interface PolarEncoding {
  readonly kind: 'polar'
  /** Field of `parsed` holding the record array. */
  readonly data: string
  /** Field of each record feeding the angle channel. */
  readonly angle: string
  /** Field of each record feeding the radius channel. */
  readonly radius: string
  /** Optional intensity channel; undeclared hides the ramp. */
  readonly intensity: string | undefined
}

export interface PolarScan {
  readonly points: readonly PolarPoint[]
  /** Elements the server dropped from the array (truncation marker sum). */
  readonly truncatedPoints: number
  /** False when the declaration bound no intensity channel (ramp is hidden). */
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

interface Invalid {
  readonly kind: 'invalid'
  /** What the declaration promised and what the data actually holds. */
  readonly reason: string
}

export type DispatchedView =
  | { readonly kind: 'polar'; readonly scan: PolarScan }
  | { readonly kind: 'generic' }
  | Invalid

function invalid(reason: string): Invalid {
  return { kind: 'invalid', reason }
}

/** Names a value in a reason string without dumping the whole payload. */
function describe(value: unknown): string {
  if (value === undefined) return 'nothing'
  if (value === null) return 'null'
  if (Array.isArray(value)) return 'an array'
  if (typeof value === 'object') return 'an object'
  if (typeof value === 'number') return String(value)
  return JSON.stringify(value)
}

function nameOf(value: unknown): string | undefined {
  return typeof value === 'string' && value !== '' ? value : undefined
}

function notAField(label: string, key: string, value: unknown): Invalid {
  return invalid(`${label}.${key} must name a field (a non-empty string) — got ${describe(value)}`)
}

/** Validates one declaration. `label` is "view" or "encoding", for the reason. */
function readEncoding(declared: unknown, label: string): PolarEncoding | Invalid {
  if (!isRecord(declared)) {
    return invalid(`${label} must be an object — got ${describe(declared)}`)
  }
  if (declared.kind !== 'polar') {
    return invalid(
      `${label}.kind ${describe(declared.kind)} is not supported — this viewer renders "polar" only`,
    )
  }
  const data = nameOf(declared.data)
  if (data === undefined) return notAField(label, 'data', declared.data)
  const angle = nameOf(declared.angle)
  if (angle === undefined) return notAField(label, 'angle', declared.angle)
  const radius = nameOf(declared.radius)
  if (radius === undefined) return notAField(label, 'radius', declared.radius)
  const rawIntensity = declared.intensity
  const intensity = rawIntensity === undefined ? undefined : nameOf(rawIntensity)
  if (rawIntensity !== undefined && intensity === undefined) {
    return notAField(label, 'intensity', rawIntensity)
  }
  return { kind: 'polar', data, angle, radius, intensity }
}

interface PointsHit {
  readonly kind: 'points'
  readonly points: readonly PolarPoint[]
  readonly truncated: number
}

function channelFailed(
  label: string,
  channel: string,
  field: string,
  value: unknown,
  index: number,
): Invalid {
  const detail =
    value === undefined
      ? `missing on element ${index}`
      : `is ${describe(value)} on element ${index} — expected a finite number`
  return invalid(`${label}.${channel} ${JSON.stringify(field)} ${detail}`)
}

function numberAt(element: JsonRecord, field: string): number | undefined {
  const value = element[field]
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

/**
 * Reads the declared array out of `parsed` and pulls each channel by the name
 * the declaration gave. A declared channel that a record does not answer is an
 * error: a chart drawn from half the records would misreport the device.
 */
function readPolarPoints(
  parsed: JsonRecord,
  encoding: PolarEncoding,
  label: string,
): PointsHit | Invalid {
  const source = `${label}.data ${JSON.stringify(encoding.data)}`
  const array = parsed[encoding.data]
  if (!Array.isArray(array)) {
    return invalid(`${source} is not an array in parsed — got ${describe(array)}`)
  }
  const points: PolarPoint[] = []
  let truncated = 0
  for (const [index, element] of array.entries()) {
    const marker = truncationMarker(element)
    if (marker !== undefined) {
      truncated += marker
      continue
    }
    if (!isRecord(element)) {
      return invalid(`${source} element ${index} is ${describe(element)} — expected an object`)
    }
    const angle = numberAt(element, encoding.angle)
    if (angle === undefined) {
      return channelFailed(label, 'angle', encoding.angle, element[encoding.angle], index)
    }
    const distance = numberAt(element, encoding.radius)
    if (distance === undefined) {
      return channelFailed(label, 'radius', encoding.radius, element[encoding.radius], index)
    }
    let intensity = 0
    if (encoding.intensity !== undefined) {
      const declared = numberAt(element, encoding.intensity)
      if (declared === undefined) {
        return channelFailed(
          label,
          'intensity',
          encoding.intensity,
          element[encoding.intensity],
          index,
        )
      }
      intensity = declared
    }
    points.push({ angleDeg: angle, distanceMm: distance, intensity })
  }
  if (points.length === 0) {
    return invalid(`${source} holds no records to plot`)
  }
  return { kind: 'points', points, truncated }
}

export function asString(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined
}

export function asNumber(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined
}

// 0x4F42 = "OB", the OBP frame magic (crates/openbaud-core frame header).
// Protocol identification, not a rendering guess: it labels the frame, and its
// absence costs nothing but the label.
const OBP_MAGIC = 0x4f42

function protocolLabel(parsed: JsonRecord): string | undefined {
  if (parsed.magic !== OBP_MAGIC) return undefined
  const version = asNumber(parsed.version)
  return version === undefined ? undefined : `OBP/${version}`
}

/**
 * @param structured full result object (`view` may declare its rendering)
 * @param override   show_result's `encoding`, which wins over `view` when set
 */
export function dispatchResult(structured: OpenbaudSummary, override?: unknown): DispatchedView {
  const overridden = override !== undefined && override !== null
  const declared = overridden ? override : structured.view
  if (declared === undefined || declared === null) return { kind: 'generic' }
  const label = overridden ? 'encoding' : 'view'

  const encoding = readEncoding(declared, label)
  if (encoding.kind === 'invalid') return encoding
  const parsed = structured.parsed
  if (!isRecord(parsed)) {
    return invalid(
      `${label} declares a ${encoding.kind} chart but the result carries no "parsed" object — got ${describe(parsed)}`,
    )
  }
  const hit = readPolarPoints(parsed, encoding, label)
  if (hit.kind === 'invalid') return hit

  return {
    kind: 'polar',
    scan: {
      points: hit.points,
      truncatedPoints: hit.truncated,
      hasIntensity: encoding.intensity !== undefined,
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
