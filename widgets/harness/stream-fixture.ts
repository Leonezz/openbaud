// Synthetic stream_poll for the harness host: deterministic frames at a fixed
// dt, parsed on the "server" side exactly like the real tool (the widget never
// decodes bytes). Mirrors engine/stream.rs stream_pull's result shape:
//   { subscription_id, session_id, frames, next_seq, dropped_frames,
//     dropped_chunks, parse, units }
// with frames { seq, ts_ms, hex, text, parsed | parse_error }.
//
// Scenario switches: `dropped` makes dropped_frames/chunks climb over time,
// `parseErrors` gives every 5th frame a parse_error instead of parsed —
// per-frame honesty, the stream itself never stops.
import type { JsonObject } from './fixtures'

const SESSION_ID = 's-live-1'
const FRAME_DT_MS = 60
const DEVICE = 'openbaud-pv-board'
const SCOPE_COMMAND = 'obp1_telemetry'
const HEATMAP_COMMAND = 'obp1_thermal'
const HEAT_ROWS = 8
const HEAT_COLS = 8

/** Live descriptor the agent would hand to show_result for the scope. */
export function scopeStreamDescriptor(): JsonObject {
  return {
    stream: {
      session_id: SESSION_ID,
      parse: { device: DEVICE, command: SCOPE_COMMAND },
    },
    view: { kind: 'scope', y: ['sine_v', 'saw_v'] },
  }
}

/** Live descriptor for the 8×8 heatmap. */
export function heatmapStreamDescriptor(): JsonObject {
  return {
    stream: {
      session_id: SESSION_ID,
      parse: { device: DEVICE, command: HEATMAP_COMMAND },
    },
    view: { kind: 'heatmap', data: 'cells', rows: HEAT_ROWS, cols: HEAT_COLS },
  }
}

/** A descriptor the viewer must reject with a named reason (5 y series). */
export function badStreamDescriptor(): JsonObject {
  return {
    stream: {
      session_id: SESSION_ID,
      parse: { device: DEVICE, command: SCOPE_COMMAND },
    },
    view: { kind: 'scope', y: ['a', 'b', 'c', 'd', 'e'] },
  }
}

export interface StreamScenarioFlags {
  readonly dropped: boolean
  readonly parseErrors: boolean
  /** Fail every call during alternating 5 s windows — retry/backoff must show. */
  readonly flaky: boolean
}

interface Subscription {
  readonly id: string
  readonly command: string
  readonly startedAt: number
  /** ts_ms (session-relative) already emitted up to, exclusive. */
  emittedMs: number
  nextSeq: number
}

let subSerial = 0
const subscriptions = new Map<string, Subscription>()

function sineValue(tMs: number): number {
  return Number((1.65 + 0.9 * Math.sin((tMs / 4000) * 2 * Math.PI)).toFixed(3))
}

function sawValue(tMs: number): number {
  return Number((3.3 * ((tMs % 2500) / 2500)).toFixed(3))
}

/** 8×8 thermal field: warm base plus a hot spot circling the grid. */
function thermalCells(tMs: number): number[] {
  const angle = (tMs / 6000) * 2 * Math.PI
  const cx = 3.5 + 2.4 * Math.cos(angle)
  const cy = 3.5 + 2.4 * Math.sin(angle)
  const cells: number[] = []
  for (let row = 0; row < HEAT_ROWS; row += 1) {
    for (let col = 0; col < HEAT_COLS; col += 1) {
      const d2 = (col - cx) ** 2 + (row - cy) ** 2
      cells.push(Number((22 + 41 * Math.exp(-d2 / 3.2)).toFixed(1)))
    }
  }
  return cells
}

function frameFor(sub: Subscription, tsMs: number, flags: StreamScenarioFlags): JsonObject {
  const seq = sub.nextSeq
  sub.nextSeq += 1
  const base: JsonObject = {
    seq,
    ts_ms: tsMs,
    hex: '4F 42 01 20',
    text: 'OB\\x01 ',
  }
  if (flags.parseErrors && seq % 5 === 4) {
    return {
      ...base,
      parse_error: `checksum mismatch on frame seq ${seq}: expected 0x1a2b, got 0x0000`,
    }
  }
  const parsed: JsonObject =
    sub.command === HEATMAP_COMMAND
      ? { seq, cells: thermalCells(tsMs) }
      : { seq, sine_v: sineValue(tsMs), saw_v: sawValue(tsMs) }
  return { ...base, parsed }
}

function unitsFor(command: string): JsonObject {
  return command === HEATMAP_COMMAND ? { cells: '°C' } : { sine_v: 'V', saw_v: 'V' }
}

function page(sub: Subscription, frames: JsonObject[], flags: StreamScenarioFlags): JsonObject {
  const elapsed = Date.now() - sub.startedAt
  return {
    subscription_id: sub.id,
    session_id: SESSION_ID,
    frames,
    next_seq: sub.nextSeq,
    page_bytes: frames.length * 9,
    oversized_frame: false,
    // Cumulative loss totals, as the real subscription reports them.
    dropped_frames: flags.dropped ? Math.floor(elapsed / 1500) : 0,
    dropped_chunks: flags.dropped ? Math.floor(elapsed / 4000) : 0,
    parse: { device: DEVICE, command: sub.command },
    units: unitsFor(sub.command),
  }
}

/**
 * One stream_poll call. Returns the result object, or throws with the same
 * loud phrasing the server uses — the host wraps the message as a tool error.
 */
export function handleStreamPoll(args: JsonObject, flags: StreamScenarioFlags): JsonObject {
  if (flags.flaky && Math.floor(Date.now() / 5000) % 2 === 1) {
    throw new Error('harness: injected transport failure (flaky 5 s window) — retry should recover')
  }
  const subscriptionId = typeof args.subscription_id === 'string' ? args.subscription_id : undefined
  const sessionId = typeof args.session_id === 'string' ? args.session_id : undefined
  const maxFrames = typeof args.max_frames === 'number' ? args.max_frames : 64

  if (subscriptionId !== undefined) {
    const sub = subscriptions.get(subscriptionId)
    if (sub === undefined) {
      throw new Error(
        `no stream subscription ${JSON.stringify(subscriptionId)} on session ${SESSION_ID} (harness: it may have been swept)`,
      )
    }
    if (args.close === true) {
      subscriptions.delete(subscriptionId)
      return { subscription_id: subscriptionId, session_id: SESSION_ID, closed: true }
    }
    const nowMs = Date.now() - sub.startedAt
    const frames: JsonObject[] = []
    while (sub.emittedMs + FRAME_DT_MS <= nowMs && frames.length < maxFrames) {
      sub.emittedMs += FRAME_DT_MS
      frames.push(frameFor(sub, sub.emittedMs, flags))
    }
    return page(sub, frames, flags)
  }

  if (sessionId === undefined) {
    throw new Error('stream_poll needs session_id (to create a subscription) or subscription_id')
  }
  if (sessionId !== SESSION_ID) {
    throw new Error(`no open session ${JSON.stringify(sessionId)} (harness serves ${SESSION_ID})`)
  }
  // Contract: parse is resolved at creation — a missing/incomplete parse block
  // is a loud error, exactly like the real server resolving response.parse.
  const parse = args.parse
  const parseObj = typeof parse === 'object' && parse !== null ? (parse as JsonObject) : undefined
  const device = typeof parseObj?.device === 'string' ? parseObj.device : undefined
  const command = typeof parseObj?.command === 'string' ? parseObj.command : undefined
  if (parseObj === undefined || device === undefined || command === undefined) {
    throw new Error(
      'stream_poll parse must declare { device, command } so frames can be parsed server-side',
    )
  }
  if (device !== DEVICE || (command !== SCOPE_COMMAND && command !== HEATMAP_COMMAND)) {
    throw new Error(
      `command ${JSON.stringify(command)} on device ${JSON.stringify(device)} has no response.parse spec in this harness — it serves ${DEVICE}/${SCOPE_COMMAND} and ${DEVICE}/${HEATMAP_COMMAND}`,
    )
  }
  subSerial += 1
  const sub: Subscription = {
    id: `sub-${subSerial}`,
    command,
    startedAt: Date.now(),
    emittedMs: 0,
    nextSeq: 0,
  }
  subscriptions.set(sub.id, sub)
  // Creation answers immediately with whatever is already buffered — at t=0
  // that is an empty page; the first frames arrive on the next pull.
  return page(sub, [], flags)
}
