// Canned tool results for the harness host. Shapes mirror the real server:
// - list_ports:        crates/openbaud/src/engine/transport.rs (PortInfo serde)
// - run_command:       crates/openbaud/src/run.rs (execute_command result)
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

// vid/pid are 4-digit uppercase hex strings; absent optionals are omitted
// (serde skip_serializing_if). serial_number values are synthetic.
export const LIST_PORTS_RESULT: JsonObject = {
  ports: [
    {
      path: '/dev/tty.usbmodem31101',
      type: 'usb',
      vid: '303A',
      pid: '1001',
      manufacturer: 'Espressif',
      product: 'ESP32-S3',
      serial_number: 'F4:12:FA:00:00:01',
    },
    {
      path: '/dev/tty.usbserial-0001',
      type: 'usb',
      vid: '10C4',
      pid: 'EA60',
      manufacturer: 'Silicon Labs',
      product: 'CP2102 USB to UART Bridge Controller',
      serial_number: '0001',
    },
    {
      path: 'mock:echo',
      type: 'mock',
      product: 'loopback echo (always available, no hardware needed)',
    },
  ],
}

// `framing` is Rust Debug output, as returned by the open tool.
export const OPEN_OK_RESULT: JsonObject = {
  session_id: 's-01',
  port: '/dev/tty.usbmodem31101',
  baud: 115200,
  framing: 'Idle { idle_ms: 30 }',
}

export const OPEN_EBUSY_MESSAGE = 'cannot open /dev/tty.usbmodem31101: Resource busy'

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

/** run_command result for scan `index` (wraps around), shaped like execute_command. */
export function radarCommandResult(index: number): JsonObject {
  const scan = scans[index % scans.length]
  if (!scan) {
    throw new Error('fixtures: radar-scans.json holds no scans')
  }
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
  }
}
