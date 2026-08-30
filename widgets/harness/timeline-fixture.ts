// Session-timeline fixture (view.kind = "timeline"): the audit-stream slice
// contract the server will produce —
//   { span:{from_ms,to_ms}, bucket_ms, events:[…], density:[…], source:{path},
//     view:{kind:"timeline"} }
// Event timestamps are anchored to the real capture's wall-clock `ts` values
// (radar-scans.json), so the eight obp1_radar_scan cmd events sit exactly
// where the hardware answered; the workflow / write / deny events around them
// are canned but shaped like .openbaud/audit.jsonl entries.
import radarScans from './radar-scans.json'
import { hexToBytes, RESULT_URI_PREFIX, type JsonObject } from './fixtures'

/** The slice of the radar-scans.json shape this fixture reads. */
interface ScanSlice {
  readonly ts: number
  readonly totalLen: number
  readonly txHex: string
}

const scans = radarScans as readonly ScanSlice[]

function scanAt(index: number): ScanSlice {
  const scan = scans[index]
  if (!scan) {
    throw new Error(`timeline-fixture: radar-scans.json has no scan ${index}`)
  }
  return scan
}

interface Burst {
  readonly ts: number
  readonly tx: number
  readonly rx: number
}

/** Sparse density buckets: byte bursts folded into bucket_ms bins. */
function densityBuckets(fromMs: number, bucketMs: number, bursts: readonly Burst[]): JsonObject[] {
  const byBucket = bursts.reduce((acc, burst) => {
    const t0 = fromMs + Math.floor((burst.ts - fromMs) / bucketMs) * bucketMs
    const prev = acc.get(t0) ?? { tx: 0, rx: 0 }
    return new Map(acc).set(t0, { tx: prev.tx + burst.tx, rx: prev.rx + burst.rx })
  }, new Map<number, { tx: number; rx: number }>())
  return [...byBucket.entries()]
    .sort(([a], [b]) => a - b)
    .map(([t0, { tx, rx }]) => ({ t0, tx_bytes: tx, rx_bytes: rx }))
}

export function timelineResult(): JsonObject {
  const first = scanAt(0)
  const last = scanAt(scans.length - 1)
  const fromMs = first.ts - 2600
  const toMs = last.ts + 3200
  const bucketMs = 200

  const scanEvents: JsonObject[] = scans.map((scan) => ({
    ts_ms: scan.ts,
    kind: 'cmd',
    tool: 'run_command',
    command: 'obp1_radar_scan',
    outcome: 'normal',
    ok: true,
  }))
  const midGap = Math.round((scanAt(3).ts + scanAt(4).ts) / 2)
  const events: JsonObject[] = [
    {
      ts_ms: first.ts - 2200,
      kind: 'workflow',
      tool: 'run_workflow',
      workflow: 'radar_selftest',
      outcome: 'normal',
      ok: true,
    },
    {
      ts_ms: first.ts - 2050,
      kind: 'workflow_step',
      tool: 'run_workflow',
      workflow: 'radar_selftest',
      command: 'obp1_status',
      outcome: 'normal',
      ok: true,
    },
    {
      ts_ms: first.ts - 1750,
      kind: 'workflow_step',
      tool: 'run_workflow',
      workflow: 'radar_selftest',
      command: 'obp1_ping',
      outcome: 'normal',
      ok: true,
    },
    { ts_ms: first.ts - 1200, kind: 'write', tool: 'send', ok: true, detail: '9 bytes raw' },
    ...scanEvents,
    {
      ts_ms: midGap,
      kind: 'deny',
      tool: 'send',
      ok: false,
      detail: 'policy: raw write refused while a capture is running',
    },
    {
      ts_ms: last.ts + 1400,
      kind: 'cmd',
      tool: 'run_command',
      command: 'obp1_status',
      outcome: 'normal',
      ok: true,
    },
  ].sort((a, b) => (a.ts_ms as number) - (b.ts_ms as number))

  const bursts: Burst[] = [
    { ts: first.ts - 2050, tx: 8, rx: 24 },
    { ts: first.ts - 1750, tx: 8, rx: 16 },
    { ts: first.ts - 1200, tx: 9, rx: 0 },
    ...scans.map((scan) => ({ ts: scan.ts, tx: hexToBytes(scan.txHex).length, rx: scan.totalLen })),
    { ts: last.ts + 1400, tx: 8, rx: 24 },
  ]

  return {
    span: { from_ms: fromMs, to_ms: toMs },
    bucket_ms: bucketMs,
    events,
    density: densityBuckets(fromMs, bucketMs, bursts),
    source: { path: '.openbaud/audit.jsonl' },
    port: '/dev/cu.usbmodem213101',
    view: { kind: 'timeline' },
  }
}

const timelineFile = (): string => `res-${scanAt(scans.length - 1).ts + 4000}-timeline.json`

function timelineJsonText(): string {
  return JSON.stringify(timelineResult(), null, 2)
}

/** show_result envelope for the timeline slice — payload rides resources/read. */
export function timelineEnvelope(): JsonObject {
  const name = timelineFile()
  return {
    source: 'file',
    path: `.openbaud/out/${name}`,
    uri: `${RESULT_URI_PREFIX}${name}`,
    bytes: new TextEncoder().encode(timelineJsonText()).length,
  }
}

/** resources/read body for the timeline URI; undefined for any other uri. */
export function timelineTextForUri(uri: string): string | undefined {
  return uri === `${RESULT_URI_PREFIX}${timelineFile()}` ? timelineJsonText() : undefined
}
