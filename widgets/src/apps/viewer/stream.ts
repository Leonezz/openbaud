// Validation for the live-stream descriptor (agent → show_result `data`) and
// for the stream_poll pages the widget pulls itself:
//
//   { stream: { session_id, parse: { device, command } },
//     view:   { kind: "scope", y: [field, …] }               // 1..=4 series
//            | { kind: "heatmap", data: field, rows, cols } }
//
// The descriptor is a *reference*, not data: the widget opens its own
// per-consumer stream_poll subscription and pulls frames the server already
// parsed (openbaud_core::exec::parse_with_spec — the widget never re-decodes
// bytes). Same honesty rule as timeline.ts: a descriptor that does not fit
// surfaces as a named invalid reason, never a silent demotion. Standalone on
// purpose (no import from dispatch.ts): dispatch imports this module.

type JsonRecord = Record<string, unknown>

export interface StreamParseRef {
  readonly device: string
  readonly command: string
}

export interface StreamRef {
  readonly sessionId: string
  readonly parse: StreamParseRef
}

/** Rolling waveform: one y series per parsed field, 1..=4 of them. */
export interface ScopeSpec {
  readonly kind: 'scope'
  readonly y: readonly string[]
}

/** rows×cols grid read from one parsed array field per frame. */
export interface HeatmapSpec {
  readonly kind: 'heatmap'
  readonly data: string
  readonly rows: number
  readonly cols: number
}

export type LiveViewSpec = ScopeSpec | HeatmapSpec

export interface StreamDescriptor {
  readonly stream: StreamRef
  readonly view: LiveViewSpec
}

export type StreamRead =
  | { readonly kind: 'stream'; readonly descriptor: StreamDescriptor }
  | { readonly kind: 'invalid'; readonly reason: string }

function invalid(reason: string): { kind: 'invalid'; reason: string } {
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

function nameAt(record: JsonRecord, key: string, where: string): string | { reason: string } {
  const value = record[key]
  return typeof value === 'string' && value !== ''
    ? value
    : { reason: `${where}.${key} must be a non-empty string — got ${describe(value)}` }
}

function positiveIntAt(record: JsonRecord, key: string, where: string): number | { reason: string } {
  const value = record[key]
  return typeof value === 'number' && Number.isInteger(value) && value > 0
    ? value
    : { reason: `${where}.${key} must be a positive integer — got ${describe(value)}` }
}

const MAX_SCOPE_SERIES = 4

function readScopeSpec(view: JsonRecord, where: string): ScopeSpec | { reason: string } {
  const y = view.y
  if (!Array.isArray(y)) {
    return { reason: `${where}.y must be an array of parsed-field names — got ${describe(y)}` }
  }
  if (y.length < 1 || y.length > MAX_SCOPE_SERIES) {
    return {
      reason: `${where}.y holds ${y.length} series — a scope draws 1 to ${MAX_SCOPE_SERIES}`,
    }
  }
  const fields: string[] = []
  for (const [index, entry] of y.entries()) {
    if (typeof entry !== 'string' || entry === '') {
      return {
        reason: `${where}.y[${index}] must name a parsed field (a non-empty string) — got ${describe(entry)}`,
      }
    }
    fields.push(entry)
  }
  return { kind: 'scope', y: fields }
}

function readHeatmapSpec(view: JsonRecord, where: string): HeatmapSpec | { reason: string } {
  const data = nameAt(view, 'data', where)
  if (typeof data !== 'string') {
    return { reason: `${where}.data must name the parsed array field — got ${describe(view.data)}` }
  }
  const rows = positiveIntAt(view, 'rows', where)
  if (typeof rows !== 'number') return rows
  const cols = positiveIntAt(view, 'cols', where)
  if (typeof cols !== 'number') return cols
  return { kind: 'heatmap', data, rows, cols }
}

function readStreamRef(value: unknown, where: string): StreamRef | { reason: string } {
  if (!isRecord(value)) {
    return {
      reason: `${where} must be an object { session_id, parse: { device, command } } — got ${describe(value)}`,
    }
  }
  const sessionId = nameAt(value, 'session_id', where)
  if (typeof sessionId !== 'string') return sessionId
  const parse = value.parse
  if (!isRecord(parse)) {
    return {
      reason: `${where}.parse must declare the command whose response spec decodes each frame ({ device, command }) — got ${describe(parse)}`,
    }
  }
  const device = nameAt(parse, 'device', `${where}.parse`)
  if (typeof device !== 'string') return device
  const command = nameAt(parse, 'command', `${where}.parse`)
  if (typeof command !== 'string') return command
  return { sessionId, parse: { device, command } }
}

/**
 * Reads the live-stream descriptor off show_result's `data` object. `label`
 * names the payload in reasons ("data" for the inline descriptor). Called by
 * dispatch whenever `structured.stream` is present or view.kind is a live
 * kind — every miss is a named reason.
 */
export function readStreamDescriptor(structured: JsonRecord, label: string): StreamRead {
  const stream = readStreamRef(structured.stream, `${label}.stream`)
  if ('reason' in stream) return invalid(stream.reason)
  const view = structured.view
  if (!isRecord(view)) {
    return invalid(
      `${label}.view must declare the live rendering ({ kind: "scope" | "heatmap", … }) — got ${describe(view)}`,
    )
  }
  const where = `${label}.view`
  if (view.kind === 'scope') {
    const spec = readScopeSpec(view, where)
    if ('reason' in spec) return invalid(spec.reason)
    return { kind: 'stream', descriptor: { stream, view: spec } }
  }
  if (view.kind === 'heatmap') {
    const spec = readHeatmapSpec(view, where)
    if ('reason' in spec) return invalid(spec.reason)
    return { kind: 'stream', descriptor: { stream, view: spec } }
  }
  return invalid(
    `${where}.kind ${describe(view.kind)} is not a live view — a stream descriptor renders "scope" or "heatmap"`,
  )
}

// ---- stream_poll page ----

export interface StreamFrame {
  readonly seq: number
  readonly tsMs: number
  /** Server-parsed field values (parse in effect and this frame decoded). */
  readonly parsed: JsonRecord | undefined
  /** Why this frame did not decode — per-frame honesty, stream continues. */
  readonly parseError: string | undefined
}

export interface StreamPage {
  readonly subscriptionId: string
  readonly frames: readonly StreamFrame[]
  readonly nextSeq: number
  readonly droppedFrames: number
  readonly droppedChunks: number
  /** parse echo from the server — present only when parse is in effect. */
  readonly parse: StreamParseRef | undefined
  /** parsed-field name → unit string, same mechanism as run_command. */
  readonly units: Readonly<Record<string, string>>
}

export type StreamPageRead =
  | { readonly kind: 'page'; readonly page: StreamPage }
  | { readonly kind: 'invalid'; readonly reason: string }

function finiteAt(record: JsonRecord, key: string, where: string): number | { reason: string } {
  const value = record[key]
  return typeof value === 'number' && Number.isFinite(value)
    ? value
    : { reason: `${where}.${key} is ${describe(value)} — expected a finite number` }
}

function readFrame(value: unknown, where: string): StreamFrame | { reason: string } {
  if (!isRecord(value)) {
    return { reason: `${where} is ${describe(value)} — expected a frame object` }
  }
  const seq = finiteAt(value, 'seq', where)
  if (typeof seq !== 'number') return seq
  const tsMs = finiteAt(value, 'ts_ms', where)
  if (typeof tsMs !== 'number') return tsMs
  const rawParsed = value.parsed
  if (rawParsed !== undefined && !isRecord(rawParsed)) {
    return { reason: `${where}.parsed is ${describe(rawParsed)} — expected an object of field values` }
  }
  const rawError = value.parse_error
  if (rawError !== undefined && typeof rawError !== 'string') {
    return { reason: `${where}.parse_error is ${describe(rawError)} — expected a string` }
  }
  if (rawParsed !== undefined && rawError !== undefined) {
    return {
      reason: `${where} carries both parsed and parse_error — a frame either decoded or it did not`,
    }
  }
  return { seq, tsMs, parsed: rawParsed, parseError: rawError }
}

function readUnits(value: unknown): Readonly<Record<string, string>> {
  if (!isRecord(value)) return {}
  const entries = Object.entries(value).filter(
    (entry): entry is [string, string] => typeof entry[1] === 'string',
  )
  return Object.fromEntries(entries)
}

/** Validates one stream_poll result (engine/stream.rs stream_pull shape). */
export function readStreamPage(structured: unknown): StreamPageRead {
  if (!isRecord(structured)) {
    return invalid(`stream_poll returned ${describe(structured)} — expected a result object`)
  }
  const subscriptionId = nameAt(structured, 'subscription_id', 'result')
  if (typeof subscriptionId !== 'string') return invalid(subscriptionId.reason)
  const rawFrames = structured.frames
  if (!Array.isArray(rawFrames)) {
    return invalid(`result.frames is ${describe(rawFrames)} — expected an array`)
  }
  const frames: StreamFrame[] = []
  for (const [index, element] of rawFrames.entries()) {
    const frame = readFrame(element, `frames[${index}]`)
    if ('reason' in frame) return invalid(frame.reason)
    frames.push(frame)
  }
  const nextSeq = finiteAt(structured, 'next_seq', 'result')
  if (typeof nextSeq !== 'number') return invalid(nextSeq.reason)
  const droppedFrames = finiteAt(structured, 'dropped_frames', 'result')
  if (typeof droppedFrames !== 'number') return invalid(droppedFrames.reason)
  const droppedChunks = finiteAt(structured, 'dropped_chunks', 'result')
  if (typeof droppedChunks !== 'number') return invalid(droppedChunks.reason)
  let parse: StreamParseRef | undefined
  if (structured.parse !== undefined) {
    const echo = readStreamRef({ session_id: '-', parse: structured.parse }, 'result')
    if ('reason' in echo) return invalid(echo.reason)
    parse = echo.parse
  }
  return {
    kind: 'page',
    page: {
      subscriptionId,
      frames,
      nextSeq,
      droppedFrames,
      droppedChunks,
      parse,
      units: readUnits(structured.units),
    },
  }
}
