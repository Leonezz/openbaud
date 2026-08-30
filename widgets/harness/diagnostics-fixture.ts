// Frame-diagnostics fixture (view.kind = "diagnostics"): the diagnose_frame
// contract over a *real* OBP/1 frame from the ESP32-S3 capture
// (radar-scans.json scan 0) whose CRC field this fixture deliberately rewrites
// to the crc16_ccitt value — the story being a profile that declared
// crc16_modbus against firmware that actually ships ccitt. Every matrix cell
// and every parse attempt below is computed here, not typed in: the checksums
// run over the actual bytes and the fixture throws if the real capture stops
// matching its own CRC (assumption drift must be loud, never canned over).
//
// Contract mirrored (src/apps/viewer/diagnostics.ts): a verified row's `at` is
// the byte offset where the stored checksum starts — frame_len − stored_len,
// where stored_len is the algorithm's byte width (ascii_hex doubles it). A
// frame whose slot the algorithm cannot run over yields an error row instead:
// kind/encoding/error, no `at`. Parse attempts carry `parsed` (structurally
// decodable at that offset — not proof the offset is right) and probe the
// server's −2..=+2 shifts, negatives assuming bytes before the frame.
import radarScans from './radar-scans.json'
import { hexToBytes, type JsonObject } from './fixtures'

interface ScanSlice {
  readonly rxHex: string
}

const scans = radarScans as readonly ScanSlice[]

function crc16Modbus(bytes: readonly number[]): [number, number] {
  let crc = 0xffff
  for (const byte of bytes) {
    crc ^= byte
    for (let i = 0; i < 8; i += 1) crc = crc & 1 ? (crc >> 1) ^ 0xa001 : crc >> 1
  }
  return [crc & 0xff, crc >> 8] // lo, hi — the order OBP/1 puts on the wire
}

function crc16Ccitt(bytes: readonly number[]): [number, number] {
  let crc = 0xffff
  for (const byte of bytes) {
    crc ^= byte << 8
    for (let i = 0; i < 8; i += 1) {
      crc = crc & 0x8000 ? ((crc << 1) ^ 0x1021) & 0xffff : (crc << 1) & 0xffff
    }
  }
  return [crc >> 8, crc & 0xff] // hi, lo — CCITT convention
}

function xor8(bytes: readonly number[]): number {
  return bytes.reduce((acc, byte) => acc ^ byte, 0)
}

function sum8(bytes: readonly number[]): number {
  return bytes.reduce((acc, byte) => (acc + byte) & 0xff, 0)
}

const hex2 = (byte: number): string => byte.toString(16).toUpperCase().padStart(2, '0')
const toHex = (bytes: readonly number[]): string => bytes.map(hex2).join(' ')

/** Declared checksum byte widths (openbaud-core ChecksumKind::len). */
const CHECKSUM_WIDTH: Record<string, number> = {
  crc16_modbus: 2,
  crc16_ccitt: 2,
  xor8: 1,
  sum8: 1,
}

interface CorruptedFrame {
  readonly frame: readonly number[]
  readonly body: readonly number[]
}

function corruptedFrame(): CorruptedFrame {
  const scan = scans[0]
  if (!scan) throw new Error('diagnostics-fixture: radar-scans.json holds no scans')
  const rx = hexToBytes(scan.rxHex)
  const n = rx.length
  const body = rx.slice(0, n - 2)
  const original = rx.slice(n - 2)
  const modbus = crc16Modbus(body)
  if (toHex(modbus) !== toHex(original)) {
    throw new Error(
      `diagnostics-fixture: crc16_modbus over the real frame body is ${toHex(modbus)} but the capture carries ${toHex(original)} — fixture assumptions broke`,
    )
  }
  // The corruption: the CRC field now carries the ccitt value instead.
  return { frame: [...body, ...crc16Ccitt(body)], body }
}

/**
 * One verified matrix row over `frame`: `at` derives from the contract
 * formula (frame_len − stored_len), never from a typed-in number, and the
 * formula self-proves — the algorithm's computed value must be exactly the
 * width the CHECKSUM_WIDTH table used for `at`, or the fixture throws.
 * `ok` is derived by comparison, never asserted.
 */
function row(kind: string, computedBytes: readonly number[], frame: readonly number[]): JsonObject {
  const width = CHECKSUM_WIDTH[kind]
  if (width === undefined) {
    throw new Error(`diagnostics-fixture: no declared width for checksum ${JSON.stringify(kind)} — cannot derive at`)
  }
  if (computedBytes.length !== width) {
    throw new Error(
      `diagnostics-fixture: ${kind} computed ${computedBytes.length} byte(s) but the width table says ${width} — the at formula would lie`,
    )
  }
  const stored = width // encoding raw: stored_len = algorithm width
  if (frame.length < stored) {
    throw new Error(
      `diagnostics-fixture: the ${frame.length}-byte frame cannot hold a ${kind} checksum (${stored} byte(s)) — this fixture expects verified raw rows`,
    )
  }
  const at = frame.length - stored
  const frameBytes = frame.slice(at)
  const ok = toHex(computedBytes) === toHex(frameBytes)
  return ok
    ? { kind, encoding: 'raw', at, ok, computed: toHex(computedBytes) }
    : { kind, encoding: 'raw', at, ok, expected: toHex(computedBytes), actual: toHex(frameBytes) }
}

/**
 * The error row, in the server's real shape: kind/encoding/error, no `at`.
 * ascii_hex stores 2 chars per checksum byte, so the slot is the last
 * 2×width bytes — and on this binary frame that slot is not ASCII hex text,
 * so the algorithm never runs. Derived, not typed in: the message names the
 * actual offending bytes, and the fixture throws if the slot unexpectedly IS
 * ascii hex (the story would silently change into a verdict row).
 */
function asciiHexErrorRow(kind: string, frame: readonly number[]): JsonObject {
  const width = CHECKSUM_WIDTH[kind]
  if (width === undefined) {
    throw new Error(`diagnostics-fixture: no declared width for checksum ${JSON.stringify(kind)} — cannot derive the ascii_hex slot`)
  }
  const stored = 2 * width // ascii_hex: two chars per checksum byte
  if (frame.length < stored) {
    return {
      kind,
      encoding: 'ascii_hex',
      error: `frame of ${frame.length} bytes is too short to carry a ${kind} checksum (${stored} byte(s))`,
    }
  }
  const at = frame.length - stored
  const slot = frame.slice(at)
  if (slot.some((byte) => byte >= 0x80)) {
    return {
      kind,
      encoding: 'ascii_hex',
      error: `ascii_hex checksum at byte ${at} is not ASCII text: ${toHex(slot)}`,
    }
  }
  const text = String.fromCharCode(...slot)
  for (let i = 0; i < width; i += 1) {
    const pair = text.slice(2 * i, 2 * i + 2)
    if (!/^[0-9a-fA-F]{2}$/.test(pair)) {
      return {
        kind,
        encoding: 'ascii_hex',
        error: `ascii_hex checksum at byte ${at}: ${JSON.stringify(pair)} is not a hex byte`,
      }
    }
  }
  throw new Error(
    `diagnostics-fixture: the ${stored}-byte tail slot ${JSON.stringify(text)} unexpectedly decodes as ASCII hex — the ${kind}/ascii_hex error row would be a lie`,
  )
}

/**
 * Reads the 2-byte big-endian magic at `offset`; parse attempt material.
 * `parsed` means the record decodes structurally at that offset — mirroring
 * the server, it is not a claim that the offset is the right one.
 */
function attemptAt(frame: readonly number[], offset: number): JsonObject {
  if (offset < 0) {
    // The server shifts every declared field by `offset`; the first field
    // (magic, declared at 0) lands before byte 0 and the probe fails.
    return {
      offset,
      parsed: false,
      error: `offset ${offset} puts field "magic" before byte 0 (declared at 0)`,
    }
  }
  const m0 = frame[offset]
  const m1 = frame[offset + 1]
  if (m0 === undefined || m1 === undefined) {
    throw new Error(`diagnostics-fixture: offset ${offset} runs past the frame`)
  }
  const magic = (m0 << 8) | m1
  if (magic !== 0x4f42) {
    return {
      offset,
      parsed: false,
      error: `magic reads 0x${magic.toString(16).toUpperCase().padStart(4, '0')} — expected 0x4F42 ("OB")`,
    }
  }
  const word = (lo: number): number => {
    const a = frame[offset + lo]
    const b = frame[offset + lo + 1]
    if (a === undefined || b === undefined) {
      throw new Error(`diagnostics-fixture: field at ${offset + lo} runs past the frame`)
    }
    return a | (b << 8)
  }
  const byteAt = (index: number): number => {
    const value = frame[offset + index]
    if (value === undefined) {
      throw new Error(`diagnostics-fixture: field at ${offset + index} runs past the frame`)
    }
    return value
  }
  const totalLen = word(6)
  const fields = {
    magic: '0x4F42',
    version: byteAt(2),
    kind: `0x${hex2(byteAt(3))}`,
    seq: word(4),
    total_len: totalLen,
    uptime_ms: word(8) | (word(10) << 16),
    point_count: byteAt(12),
  }
  const parsed = totalLen === frame.length - offset
  return parsed
    ? { offset, parsed, fields }
    : { offset, parsed: false, error: `total_len reads ${totalLen} but only ${frame.length - offset} bytes remain` }
}

/** The diagnose_frame result: full payload, view declares the rendering. */
export function diagnosticsResult(): JsonObject {
  const { frame, body } = corruptedFrame()
  const n = frame.length
  return {
    device: 'openbaud-pv-board',
    command: 'obp1_radar_scan',
    port: '/dev/cu.usbmodem213101',
    outcome: 'checksum_error',
    hex: toHex(frame),
    frame_len: n,
    checksum_matrix: [
      row('crc16_modbus', crc16Modbus(body), frame),
      asciiHexErrorRow('crc16_modbus', frame),
      row('crc16_ccitt', crc16Ccitt(body), frame),
      row('xor8', [xor8(frame.slice(0, n - 1))], frame),
      row('sum8', [sum8(frame.slice(0, n - 1))], frame),
    ],
    parse_attempts: [-2, -1, 0, 1, 2].map((offset) => attemptAt(frame, offset)),
    view: { kind: 'diagnostics' },
  }
}
