// Validation for the `view.kind === "timeline"` result payload (server
// contract, mirrored by harness/fixtures.ts):
//
//   { span: { from_ms, to_ms }, bucket_ms,
//     events:  [{ ts_ms, kind, tool, command?, workflow?, outcome?, ok, detail? }],
//     density: [{ t0, tx_bytes, rx_bytes }],
//     source:  { path }, view: { kind: "timeline" } }
//
// Same honesty rule as the polar path in dispatch.ts: a declared timeline that
// does not fit its data surfaces as an explicit reason naming the field and
// element — never a silent demotion to the key/value card. Standalone on
// purpose (no import from dispatch.ts): dispatch imports this module, and a
// cycle between the two would only save a few one-line predicates.
import type { DensityBucket, EventMarkerKind } from '../../render/overlays'

type JsonRecord = Record<string, unknown>

export const TIMELINE_EVENT_KINDS = ['cmd', 'write', 'deny', 'workflow', 'workflow_step'] as const
export type TimelineEventKind = EventMarkerKind

export interface TimelineEvent {
  readonly tsMs: number
  readonly kind: TimelineEventKind
  readonly tool: string
  readonly command: string | undefined
  readonly workflow: string | undefined
  readonly outcome: string | undefined
  readonly ok: boolean
  readonly detail: string | undefined
}

export interface TimelineData {
  readonly fromMs: number
  readonly toMs: number
  readonly bucketMs: number
  readonly events: readonly TimelineEvent[]
  readonly density: readonly DensityBucket[]
  /** Elements the server summarizer dropped (truncation marker sums). */
  readonly truncatedEvents: number
  readonly truncatedBuckets: number
  /** Where the events came from (e.g. .openbaud/audit.jsonl). */
  readonly sourcePath: string
  /** True when the result's port names a replay transport. */
  readonly replay: boolean
}

export type TimelineRead =
  | { readonly kind: 'timeline'; readonly timeline: TimelineData }
  | { readonly kind: 'invalid'; readonly reason: string }

function invalid(reason: string): TimelineRead {
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

function finiteAt(record: JsonRecord, key: string, where: string): number | string {
  const value = record[key]
  return typeof value === 'number' && Number.isFinite(value)
    ? value
    : `${where}.${key} is ${describe(value)} — expected a finite number`
}

function optionalStringAt(record: JsonRecord, key: string, where: string): string | undefined | { reason: string } {
  const value = record[key]
  if (value === undefined) return undefined
  return typeof value === 'string' ? value : { reason: `${where}.${key} is ${describe(value)} — expected a string` }
}

interface EventsHit {
  readonly events: readonly TimelineEvent[]
  readonly truncated: number
}

function readEvents(value: unknown, where: string): EventsHit | { reason: string } {
  if (!Array.isArray(value)) return { reason: `${where} is ${describe(value)} — expected an array` }
  const events: TimelineEvent[] = []
  let truncated = 0
  for (const [index, element] of value.entries()) {
    const marker = truncationOf(element)
    if (marker !== undefined) {
      truncated += marker
      continue
    }
    const at = `${where}[${index}]`
    if (!isRecord(element)) return { reason: `${at} is ${describe(element)} — expected an object` }
    const tsMs = finiteAt(element, 'ts_ms', at)
    if (typeof tsMs === 'string') return { reason: tsMs }
    const kind = element.kind
    if (typeof kind !== 'string' || !(TIMELINE_EVENT_KINDS as readonly string[]).includes(kind)) {
      return { reason: `${at}.kind is ${describe(kind)} — expected one of ${TIMELINE_EVENT_KINDS.join('/')}` }
    }
    if (typeof element.tool !== 'string') {
      return { reason: `${at}.tool is ${describe(element.tool)} — expected a string` }
    }
    if (typeof element.ok !== 'boolean') {
      return { reason: `${at}.ok is ${describe(element.ok)} — expected a boolean` }
    }
    const optionals: Partial<Record<'command' | 'workflow' | 'outcome' | 'detail', string>> = {}
    for (const key of ['command', 'workflow', 'outcome', 'detail'] as const) {
      const read = optionalStringAt(element, key, at)
      if (typeof read === 'object' && read !== null) return read
      if (read !== undefined) optionals[key] = read
    }
    events.push({
      tsMs,
      kind: kind as TimelineEventKind,
      tool: element.tool,
      ok: element.ok,
      command: optionals.command,
      workflow: optionals.workflow,
      outcome: optionals.outcome,
      detail: optionals.detail,
    })
  }
  return { events, truncated }
}

interface DensityHit {
  readonly buckets: readonly DensityBucket[]
  readonly truncated: number
}

function readDensity(value: unknown, where: string): DensityHit | { reason: string } {
  if (!Array.isArray(value)) return { reason: `${where} is ${describe(value)} — expected an array` }
  const buckets: DensityBucket[] = []
  let truncated = 0
  for (const [index, element] of value.entries()) {
    const marker = truncationOf(element)
    if (marker !== undefined) {
      truncated += marker
      continue
    }
    const at = `${where}[${index}]`
    if (!isRecord(element)) return { reason: `${at} is ${describe(element)} — expected an object` }
    const t0 = finiteAt(element, 't0', at)
    if (typeof t0 === 'string') return { reason: t0 }
    const txBytes = finiteAt(element, 'tx_bytes', at)
    if (typeof txBytes === 'string') return { reason: txBytes }
    const rxBytes = finiteAt(element, 'rx_bytes', at)
    if (typeof rxBytes === 'string') return { reason: rxBytes }
    buckets.push({ t0, txBytes, rxBytes })
  }
  return { buckets, truncated }
}

/**
 * Reads the timeline payload off the full result object. `label` names the
 * declaration source ("view" or "encoding") in reasons; `replay` is computed
 * by dispatch from the result's `port` field.
 */
export function readTimeline(structured: JsonRecord, label: string, replay: boolean): TimelineRead {
  const span = structured.span
  if (!isRecord(span)) {
    return invalid(`${label} declares a timeline but the result's span is ${describe(span)} — expected an object`)
  }
  const fromMs = finiteAt(span, 'from_ms', 'span')
  if (typeof fromMs === 'string') return invalid(fromMs)
  const toMs = finiteAt(span, 'to_ms', 'span')
  if (typeof toMs === 'string') return invalid(toMs)
  if (toMs <= fromMs) {
    return invalid(`span.to_ms (${toMs}) is not after span.from_ms (${fromMs}) — an empty span has no axis`)
  }
  const bucketMs = finiteAt(structured, 'bucket_ms', 'result')
  if (typeof bucketMs === 'string') return invalid(bucketMs)
  if (bucketMs <= 0) {
    return invalid(`result.bucket_ms is ${bucketMs} — expected a positive bucket width`)
  }
  const events = readEvents(structured.events, 'events')
  if ('reason' in events) return invalid(events.reason)
  const density = readDensity(structured.density, 'density')
  if ('reason' in density) return invalid(density.reason)
  const source = structured.source
  if (!isRecord(source) || typeof source.path !== 'string' || source.path === '') {
    return invalid(
      `result.source must carry the data-source path (an object with a non-empty "path") — got ${describe(source)}`,
    )
  }
  return {
    kind: 'timeline',
    timeline: {
      fromMs,
      toMs,
      bucketMs,
      events: events.events,
      density: density.buckets,
      truncatedEvents: events.truncated,
      truncatedBuckets: density.truncated,
      sourcePath: source.path,
      replay,
    },
  }
}
