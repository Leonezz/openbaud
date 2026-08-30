// Validation for the `view.kind === "capture"` result payload (the
// capture_frames tool contract, mirrored by harness/capture-fixture.ts):
//
//   { header: { port, started_ms, path?, session?, note? },
//     frames: [{ seq, ts_ms, dir: "tx"|"rx", len, hex }],
//     total_in_window, view: { kind: "capture" } }
//
// `header` mirrors the .obcap header line (crates/openbaud/src/engine/
// capture.rs). `total_in_window` counts every frame the requested window held;
// `frames` is the slice the agent chose to deliver — the card must state both
// and never pretend the slice is the window. Same honesty rule as timeline.ts:
// a declared capture view that does not fit its data surfaces as an explicit
// reason — never a silent demotion. Standalone on purpose: dispatch.ts
// imports this module.
import type { DensityBucket } from '../../render/overlays'
import { parseHexBytes } from './diagnostics'

type JsonRecord = Record<string, unknown>

export interface CaptureHeader {
  readonly port: string
  readonly startedMs: number
  readonly path: string | undefined
  readonly session: string | undefined
  readonly note: string | undefined
}

export interface CaptureFrame {
  readonly seq: number
  readonly tsMs: number
  readonly dir: 'tx' | 'rx'
  readonly len: number
  readonly bytes: readonly number[]
}

export interface CaptureData {
  readonly header: CaptureHeader
  readonly frames: readonly CaptureFrame[]
  /** Elements the server summarizer dropped (truncation marker sums). */
  readonly truncatedFrames: number
  /** Frames the requested window actually held (>= frames delivered). */
  readonly totalInWindow: number
  /** Plot window; undefined when no frames were delivered. */
  readonly fromMs: number | undefined
  readonly toMs: number | undefined
  readonly bucketMs: number | undefined
  readonly density: readonly DensityBucket[]
  /** True when the result's port names a replay transport. */
  readonly replay: boolean
}

export type CaptureRead =
  | { readonly kind: 'capture'; readonly capture: CaptureData }
  | { readonly kind: 'invalid'; readonly reason: string }

function invalid(reason: string): CaptureRead {
  return { kind: 'invalid', reason }
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function describe(value: unknown): string {
  if (value === undefined) return 'nothing'
  if (value === null) return 'null'
  if (Array.isArray(value)) return 'an array'
  if (typeof value === 'object') return 'an object'
  if (typeof value === 'number') return String(value)
  return JSON.stringify(value)
}

/** output.rs replaces long array tails with a single `{"truncated": N}`. */
function truncationOf(value: unknown): number | undefined {
  if (!isRecord(value)) return undefined
  const keys = Object.keys(value)
  return keys.length === 1 && keys[0] === 'truncated' && typeof value.truncated === 'number'
    ? value.truncated
    : undefined
}

function finiteAt(record: JsonRecord, key: string, where: string): number | { reason: string } {
  const value = record[key]
  return typeof value === 'number' && Number.isFinite(value)
    ? value
    : { reason: `${where}.${key} is ${describe(value)} — expected a finite number` }
}

function readHeader(value: unknown): CaptureHeader | { reason: string } {
  if (!isRecord(value)) {
    return { reason: `result.header is ${describe(value)} — expected the capture header object` }
  }
  const port = value.port
  if (typeof port !== 'string' || port === '') {
    return { reason: `header.port is ${describe(port)} — expected a non-empty string` }
  }
  const startedMs = finiteAt(value, 'started_ms', 'header')
  if (typeof startedMs === 'object') return startedMs
  const optionals: Partial<Record<'path' | 'session' | 'note', string>> = {}
  for (const key of ['path', 'session', 'note'] as const) {
    const raw = value[key]
    if (raw === undefined || raw === null) continue
    if (typeof raw !== 'string') {
      return { reason: `header.${key} is ${describe(raw)} — expected a string` }
    }
    optionals[key] = raw
  }
  return {
    port,
    startedMs,
    path: optionals.path,
    session: optionals.session,
    note: optionals.note,
  }
}

interface FramesHit {
  readonly frames: readonly CaptureFrame[]
  readonly truncated: number
}

function readFrames(value: unknown): FramesHit | { reason: string } {
  if (!Array.isArray(value)) {
    return { reason: `result.frames is ${describe(value)} — expected an array` }
  }
  const frames: CaptureFrame[] = []
  let truncated = 0
  for (const [index, element] of value.entries()) {
    const marker = truncationOf(element)
    if (marker !== undefined) {
      truncated += marker
      continue
    }
    const at = `frames[${index}]`
    if (!isRecord(element)) return { reason: `${at} is ${describe(element)} — expected an object` }
    const seq = finiteAt(element, 'seq', at)
    if (typeof seq === 'object') return seq
    const tsMs = finiteAt(element, 'ts_ms', at)
    if (typeof tsMs === 'object') return tsMs
    const dir = element.dir
    if (dir !== 'tx' && dir !== 'rx') {
      return { reason: `${at}.dir is ${describe(dir)} — expected "tx" or "rx"` }
    }
    const len = finiteAt(element, 'len', at)
    if (typeof len === 'object') return len
    const hex = element.hex
    if (typeof hex !== 'string') {
      return { reason: `${at}.hex is ${describe(hex)} — expected the frame bytes as a hex string` }
    }
    const bytes = parseHexBytes(hex, `${at}.hex`)
    if ('reason' in bytes) return bytes
    if (bytes.length !== len) {
      return { reason: `${at}.len says ${len} but hex holds ${bytes.length} bytes — the frame contradicts itself` }
    }
    frames.push({ seq, tsMs, dir, len, bytes })
  }
  return { frames, truncated }
}

/**
 * Bucket width that keeps the density readable for any window: the smallest
 * 1/2/5 x 10^k step giving at most ~140 buckets across the span.
 */
export function adaptiveBucketMs(spanMs: number, targetBuckets = 140): number {
  const raw = Math.max(1, spanMs / targetBuckets)
  let magnitude = 1
  while (magnitude * 10 <= raw) magnitude *= 10
  for (const step of [1, 2, 5]) {
    if (magnitude * step >= raw) return magnitude * step
  }
  return magnitude * 10
}

function buildDensity(
  frames: readonly CaptureFrame[],
  fromMs: number,
  bucketMs: number,
): DensityBucket[] {
  const byBucket = new Map<number, { tx: number; rx: number }>()
  for (const frame of frames) {
    const t0 = fromMs + Math.floor((frame.tsMs - fromMs) / bucketMs) * bucketMs
    const prev = byBucket.get(t0) ?? { tx: 0, rx: 0 }
    byBucket.set(t0, {
      tx: prev.tx + (frame.dir === 'tx' ? frame.len : 0),
      rx: prev.rx + (frame.dir === 'rx' ? frame.len : 0),
    })
  }
  return [...byBucket.entries()]
    .sort(([a], [b]) => a - b)
    .map(([t0, { tx, rx }]) => ({ t0, txBytes: tx, rxBytes: rx }))
}

/**
 * Reads the capture payload off the full result object. `label` names the
 * declaration source ("view" or "encoding") in reasons; `replay` is computed
 * by dispatch from the result's `port` field.
 */
export function readCapture(structured: JsonRecord, label: string, replay: boolean): CaptureRead {
  const header = readHeader(structured.header)
  if ('reason' in header) {
    return invalid(`${label} declares a capture but ${header.reason}`)
  }
  const hit = readFrames(structured.frames)
  if ('reason' in hit) return invalid(hit.reason)
  const totalInWindow = finiteAt(structured, 'total_in_window', 'result')
  if (typeof totalInWindow === 'object') return invalid(totalInWindow.reason)
  if (totalInWindow < hit.frames.length) {
    return invalid(
      `result.total_in_window says ${totalInWindow} but frames delivers ${hit.frames.length} — the window cannot hold fewer frames than were delivered`,
    )
  }

  if (hit.frames.length === 0) {
    return {
      kind: 'capture',
      capture: {
        header,
        frames: hit.frames,
        truncatedFrames: hit.truncated,
        totalInWindow,
        fromMs: undefined,
        toMs: undefined,
        bucketMs: undefined,
        density: [],
        replay,
      },
    }
  }

  let minTs = Infinity
  let maxTs = -Infinity
  for (const frame of hit.frames) {
    if (frame.tsMs < minTs) minTs = frame.tsMs
    if (frame.tsMs > maxTs) maxTs = frame.tsMs
  }
  const bucketMs = adaptiveBucketMs(Math.max(1, maxTs - minTs))
  // One trailing bucket keeps the last frame's bar inside the axis.
  const toMs = maxTs + bucketMs
  return {
    kind: 'capture',
    capture: {
      header,
      frames: hit.frames,
      truncatedFrames: hit.truncated,
      totalInWindow,
      fromMs: minTs,
      toMs,
      bucketMs,
      density: buildDensity(hit.frames, minTs, bucketMs),
      replay,
    },
  }
}
