// Canned tool results for the harness host. Shapes mirror the real server:
// - list_ports:        crates/openbaud/src/engine/transport.rs (PortInfo serde)
// - run_command:       crates/openbaud/src/run.rs (execute_command result),
//                      including the optional top-level `view` declaring how
//                      the result is drawn
// - show_result:       tiny envelope { source, path?, uri?, bytes?, encoding? };
//                      the full result JSON is served over resources/read
//                      instead, and `encoding` (when the agent decoded the
//                      bytes itself) overrides the result's own `view`
// - tools/call envelope: crates/openbaud/src/mcp/mod.rs
// radar-scans.json is real ESP32-S3 capture data (media/pv/src/radar-scans.json).
import radarScans from './radar-scans.json'

export type JsonObject = Record<string, unknown>

interface RadarScan {
  ts: number
  seq: number
  uptimeMs: number
  totalLen: number
  simulatedScene: number
  crcValid: boolean
  txHex: string
  rxHex: string
  points: readonly { angleDeg: number; distanceMm: number; intensity: number }[]
}

const scans = radarScans as readonly RadarScan[]

/** Same envelope as the Rust server: pretty text for the model, structuredContent for the widget. */
export function wrapToolResult(result: JsonObject): JsonObject {
  return {
    content: [{ type: 'text', text: JSON.stringify(result, null, 2) }],
    structuredContent: result,
  }
}

export function wrapToolError(message: string): JsonObject {
  return {
    content: [{ type: 'text', text: `error: ${message}` }],
    isError: true,
  }
}

// ask_port candidates: the measured macOS enumeration (9 nodes = 4 physical
// devices + mock:echo), plus the enrichment ask_port adds on top of PortInfo:
//   matches_devices — workspace devices whose selector hits this port
//   open_session    — session already holding it (row cannot be picked)
//   alias_of        — /dev/tty.X twin of the canonical /dev/cu.X
// vid/pid are 4-digit uppercase hex; absent optionals are omitted keys
// (serde skip_serializing_if), so no serial_number is invented here.
const ASK_PORT_CANDIDATES: readonly JsonObject[] = [
  {
    path: '/dev/cu.usbmodem213101',
    type: 'usb',
    vid: '303A',
    pid: '1001',
    manufacturer: 'Espressif',
    matches_devices: ['openbaud-pv-board'],
  },
  {
    path: '/dev/tty.usbmodem213101',
    type: 'usb',
    vid: '303A',
    pid: '1001',
    manufacturer: 'Espressif',
    matches_devices: ['openbaud-pv-board'],
    alias_of: '/dev/cu.usbmodem213101',
  },
  // Held by another session: both nodes of the device are blocked.
  {
    path: '/dev/cu.usbmodem5AF61139901',
    type: 'usb',
    vid: '1A86',
    pid: '55D3',
    product: 'USB Single Serial',
    open_session: 's-2',
  },
  {
    path: '/dev/tty.usbmodem5AF61139901',
    type: 'usb',
    vid: '1A86',
    pid: '55D3',
    product: 'USB Single Serial',
    open_session: 's-2',
    alias_of: '/dev/cu.usbmodem5AF61139901',
  },
  { path: '/dev/cu.debug-console', type: 'native' },
  { path: '/dev/tty.debug-console', type: 'native', alias_of: '/dev/cu.debug-console' },
  { path: '/dev/cu.Bluetooth-Incoming-Port', type: 'bluetooth' },
  {
    path: '/dev/tty.Bluetooth-Incoming-Port',
    type: 'bluetooth',
    alias_of: '/dev/cu.Bluetooth-Incoming-Port',
  },
  {
    path: 'mock:echo',
    type: 'mock',
    product: 'loopback echo (always available, no hardware needed)',
  },
]

export const ASK_PORT_RESULT: JsonObject = {
  reason: 'No serial port is bound to openbaud-pv-board in this workspace yet.',
  device: 'openbaud-pv-board',
  candidates: ASK_PORT_CANDIDATES,
}

export const ASK_PORT_INPUT: JsonObject = {
  device: 'openbaud-pv-board',
  reason: 'No serial port is bound to openbaud-pv-board in this workspace yet.',
}

// list_ports carries no UI binding, but it does carry the same enrichment as
// ask_port's candidates (tools::enriched_ports in crates/openbaud/src/mcp/tools.rs
// feeds both). Rescan therefore keeps the match/session/alias information.
export const LIST_PORTS_RESULT: JsonObject = {
  ports: ASK_PORT_CANDIDATES,
}

function hexToBytes(hex: string): number[] {
  return hex
    .trim()
    .split(/\s+/)
    .map((token) => {
      const byte = Number.parseInt(token, 16)
      if (Number.isNaN(byte)) {
        throw new Error(`fixtures: invalid hex byte ${JSON.stringify(token)}`)
      }
      return byte
    })
}

// Port of openbaud-core hex::to_text_lossy.
function toTextLossy(bytes: readonly number[]): string {
  return bytes
    .map((b) => {
      if (b === 0x0a) return '\\n'
      if (b === 0x0d) return '\\r'
      if (b === 0x09) return '\\t'
      if (b >= 0x20 && b <= 0x7e) return String.fromCharCode(b)
      return `\\x${b.toString(16).toUpperCase().padStart(2, '0')}`
    })
    .join('')
}

export function radarScanCount(): number {
  return scans.length
}

function scanAt(index: number): RadarScan {
  const scan = scans[index % scans.length]
  if (!scan) {
    throw new Error('fixtures: radar-scans.json holds no scans')
  }
  return scan
}

const RESULT_URI_PREFIX = 'openbaud://result/'

function resultFileName(scan: RadarScan): string {
  return `res-${scan.ts}-run_command.json`
}

/**
 * The show_result envelope for scan `index`: what the tool actually returns —
 * small on purpose, so the model context never carries the 36 points.
 */
export function showResultEnvelope(index: number): JsonObject {
  const scan = scanAt(index)
  const name = resultFileName(scan)
  const text = resultJsonText(index)
  return {
    source: 'file',
    path: `.openbaud/out/${name}`,
    uri: `${RESULT_URI_PREFIX}${name}`,
    bytes: new TextEncoder().encode(text).length,
  }
}

/** Envelope for a result small enough to travel in show_result's own input. */
export const SHOW_RESULT_INLINE: JsonObject = { source: 'inline' }

/**
 * Envelope for the agent-decoded case: the payload carries no `view`, so
 * show_result declares the mapping itself. The field names below are this
 * imaginary device's own — nothing in the viewer knows them.
 */
export const SHOW_RESULT_INLINE_ENCODED: JsonObject = {
  source: 'inline',
  encoding: {
    kind: 'polar',
    data: 'returns',
    angle: 'bearing',
    radius: 'range_mm',
    intensity: 'quality',
  },
}

function resultJsonText(index: number): string {
  return JSON.stringify(radarCommandResult(index), null, 2)
}

/** resources/read body: the complete result object, all 36 points, as text. */
export function resultTextForUri(uri: string): string | undefined {
  const index = scans.findIndex((scan) => uri === `${RESULT_URI_PREFIX}${resultFileName(scan)}`)
  return index < 0 ? undefined : resultJsonText(index)
}

/**
 * How obp1_radar_scan declares its rendering: mark kind plus one field name
 * per visual channel, all of them this device's own parse field names
 * (examples/pv-demo/devices/openbaud-pv-board/commands/obp1_radar_scan.yaml).
 */
const RADAR_VIEW: JsonObject = {
  kind: 'polar',
  data: 'points',
  angle: 'angle_deg',
  radius: 'distance_mm',
  intensity: 'intensity',
}

/** run_command result for scan `index` (wraps around), shaped like execute_command. */
export function radarCommandResult(index: number): JsonObject {
  const scan = scanAt(index)
  const rx = hexToBytes(scan.rxHex)
  const [m0, m1, version, kind] = rx
  if (m0 === undefined || m1 === undefined || version === undefined || kind === undefined) {
    throw new Error(`fixtures: scan seq ${scan.seq} rxHex is shorter than the OBP/1 header`)
  }
  return {
    device: 'openbaud-pv-board',
    command: 'obp1_radar_scan',
    tx_hex: scan.txHex,
    outcome: 'normal',
    // obp1_radar_scan declares validate: { checksum: crc16_modbus }, so the
    // server reports the algorithm it ran. A command without `validate` omits
    // this key and the viewer then claims no verification.
    checksum: 'crc16_modbus',
    expect: 'normal',
    expect_met: true,
    raw_hex: scan.rxHex,
    raw_text: toTextLossy(rx),
    parsed: {
      magic: (m0 << 8) | m1,
      version,
      kind,
      seq: scan.seq,
      total_len: scan.totalLen,
      uptime_ms: scan.uptimeMs,
      point_count: scan.points.length,
      simulated_scene: scan.simulatedScene,
      points: scan.points.map((p) => ({
        angle_deg: p.angleDeg,
        distance_mm: p.distanceMm,
        intensity: p.intensity,
      })),
    },
    units: { total_len: 'bytes', uptime_ms: 'ms' },
    view: RADAR_VIEW,
  }
}

/** The same result with no `view` at all: the generic key/value card. */
export function undeclaredCommandResult(index: number): JsonObject {
  const { view: _view, ...rest } = radarCommandResult(index)
  return rest
}

/**
 * A `view` whose angle channel names a field the records do not carry. The
 * viewer must say so rather than quietly fall back to the key/value card.
 */
export function brokenViewResult(index: number): JsonObject {
  return {
    ...radarCommandResult(index),
    view: { ...RADAR_VIEW, angle: 'bearing' },
  }
}

/**
 * The agent-decoded payload for SHOW_RESULT_INLINE_ENCODED: the same measured
 * points under a foreign vocabulary (returns/bearing/range_mm/quality) and no
 * `view` of its own. It renders identically, which is the whole point — the
 * viewer reads the declaration, never the field names.
 */
export function aliasScanResult(index: number): JsonObject {
  const scan = scanAt(index)
  return {
    device: 'thirdparty-lidar',
    command: 'sweep',
    outcome: 'normal',
    // Deliberately no `checksum`: this device declares no validate block, so
    // nothing was verified and the meta strip must stay silent about it.

    parsed: {
      seq: scan.seq,
      uptime_ms: scan.uptimeMs,
      point_count: scan.points.length,
      simulated_scene: scan.simulatedScene,
      returns: scan.points.map((p) => ({
        bearing: p.angleDeg,
        range_mm: p.distanceMm,
        quality: p.intensity,
      })),
    },
    units: { uptime_ms: 'ms' },
  }
}
