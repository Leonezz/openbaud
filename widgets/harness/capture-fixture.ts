// Capture-window fixture (view.kind = "capture"): the capture_frames contract
// over the real ESP32-S3 transaction rhythm (radar-scans.json). Each scan
// contributes its actual TX poll and RX response bytes; the TX timestamp is
// the capture's own wall clock and the RX reply is offset by a canned-but-
// plausible response latency (the source capture records one timestamp per
// transaction). The window claims 64 frames and delivers these 16 — the card
// must state that ratio, not hide it.
import radarScans from './radar-scans.json'
import { hexToBytes, RESULT_URI_PREFIX, type JsonObject } from './fixtures'

interface ScanSlice {
  readonly ts: number
  readonly txHex: string
  readonly rxHex: string
}

const scans = radarScans as readonly ScanSlice[]

function firstScan(): ScanSlice {
  const scan = scans[0]
  if (!scan) throw new Error('capture-fixture: radar-scans.json holds no scans')
  return scan
}

/** Frame seq numbers place the delivered slice inside the larger window. */
const FIRST_SEQ = 240
const TOTAL_IN_WINDOW = 64

function captureFrames(): JsonObject[] {
  return scans.flatMap((scan, index) => {
    const txBytes = hexToBytes(scan.txHex)
    const rxBytes = hexToBytes(scan.rxHex)
    return [
      {
        seq: FIRST_SEQ + index * 2,
        ts_ms: scan.ts,
        dir: 'tx',
        len: txBytes.length,
        hex: scan.txHex,
      },
      {
        seq: FIRST_SEQ + index * 2 + 1,
        // Response latency: the source capture stamps the transaction once.
        ts_ms: scan.ts + 15 + (index % 4),
        dir: 'rx',
        len: rxBytes.length,
        hex: scan.rxHex,
      },
    ]
  })
}

export function captureResult(): JsonObject {
  return {
    device: 'openbaud-pv-board',
    port: '/dev/cu.usbmodem213101',
    header: {
      path: '.openbaud/captures/pv-radar-demo.obcap',
      port: '/dev/cu.usbmodem213101',
      session: 's-1',
      note: 'pv radar demo capture',
      started_ms: firstScan().ts - 1500,
    },
    frames: captureFrames(),
    total_in_window: TOTAL_IN_WINDOW,
    view: { kind: 'capture' },
  }
}

const captureFile = (): string => `res-${firstScan().ts + 9000}-capture_frames.json`

function captureJsonText(): string {
  return JSON.stringify(captureResult(), null, 2)
}

/** show_result envelope for the capture window — payload rides resources/read. */
export function captureEnvelope(): JsonObject {
  const name = captureFile()
  return {
    source: 'file',
    path: `.openbaud/out/${name}`,
    uri: `${RESULT_URI_PREFIX}${name}`,
    bytes: new TextEncoder().encode(captureJsonText()).length,
  }
}

/** resources/read body for the capture URI; undefined for any other uri. */
export function captureTextForUri(uri: string): string | undefined {
  return uri === `${RESULT_URI_PREFIX}${captureFile()}` ? captureJsonText() : undefined
}
