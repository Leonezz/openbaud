// Validation for the `view.kind === "diagnostics"` result payload (the
// diagnose_frame tool contract, mirrored by harness/diagnostics-fixture.ts):
//
//   { hex, frame_len,
//     checksum_matrix: [
//       { kind, encoding, at, ok, expected?, actual?, computed? }  // verified
//       | { kind, encoding, error, at? }        // algorithm not applicable
//     ],
//     parse_attempts?: [{ offset, parsed, fields? | error }],
//     view: { kind: "diagnostics" } }
//
// `at` is the byte offset where the stored checksum starts, always
// frame_len − stored_len (raw: the algorithm's byte width; ascii_hex: twice
// that — two chars per byte). A frame that cannot hold the algorithm's bytes
// yields an error row instead: kind/encoding/error with `at` omitted.
// `parsed` says the bytes *decode structurally* at that offset — it is not
// proof the offset is right; the probed offsets (the server tries −2..=+2,
// negatives assume bytes before the frame) are mutually exclusive hypotheses.
// `expected`/`actual` follow the server's checksum vocabulary
// (openbaud-core CoreError::ChecksumMismatch): expected = the value the
// algorithm computed over the body, actual = the bytes the frame carries at
// `at`. `computed` is the agreed value on an ok row. Same honesty rule as
// timeline.ts: a declared diagnostics view that does not fit its data surfaces
// as an explicit reason naming the field and element — never a silent
// demotion. Standalone on purpose: dispatch.ts imports this module.

type JsonRecord = Record<string, unknown>

/** A checksum hypothesis the tool actually verified against the frame. */
export interface ChecksumVerdict {
  readonly kind: string
  readonly encoding: string
  /** Where the stored checksum starts: frame_len − stored_len, by contract. */
  readonly at: number
  readonly ok: boolean
  readonly expected: string | undefined
  readonly actual: string | undefined
  readonly computed: string | undefined
  readonly error?: undefined
}

/**
 * A hypothesis the tool could not run on this frame (too short for the
 * algorithm's bytes, non-ASCII slot, …). Carries the reason instead of a
 * verdict; `at` is normally omitted and the row never draws a footprint.
 */
export interface ChecksumError {
  readonly kind: string
  readonly encoding: string
  readonly at: number | undefined
  readonly ok: false
  readonly error: string
  readonly expected?: undefined
  readonly actual?: undefined
  readonly computed?: undefined
}

export type ChecksumRow = ChecksumVerdict | ChecksumError

export interface ParseAttempt {
  readonly offset: number
  /** Structurally decodable at this offset — not proof the offset is right. */
  readonly parsed: boolean
  /** Decoded fields — present exactly when parsed. */
  readonly fields: JsonRecord | undefined
  /** Why the decode failed — present exactly when not parsed. */
  readonly error: string | undefined
}

export interface DiagnosticsData {
  readonly bytes: readonly number[]
  readonly frameLen: number
  readonly matrix: readonly ChecksumRow[]
  /** Absent (not empty) when the tool ran no offset scan. */
  readonly attempts: readonly ParseAttempt[] | undefined
  readonly device: string | undefined
  readonly command: string | undefined
  readonly port: string | undefined
  readonly replay: boolean
}

export type DiagnosticsRead =
  | { readonly kind: 'diagnostics'; readonly diagnostics: DiagnosticsData }
  | { readonly kind: 'invalid'; readonly reason: string }

function invalid(reason: string): DiagnosticsRead {
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

/** Strict space-separated hex parse; a bad token is a reason, not a guess. */
export function parseHexBytes(hex: string, where: string): number[] | { reason: string } {
  const tokens = hex.trim().split(/\s+/)
  if (tokens.length === 1 && tokens[0] === '') {
    return { reason: `${where} holds no bytes` }
  }
  const bytes: number[] = []
  for (const [index, token] of tokens.entries()) {
    if (!/^[0-9a-fA-F]{2}$/.test(token)) {
      return { reason: `${where} token ${index} is ${JSON.stringify(token)} — expected a 2-digit hex byte` }
    }
    bytes.push(Number.parseInt(token, 16))
  }
  return bytes
}

function finiteAt(record: JsonRecord, key: string, where: string): number | { reason: string } {
  const value = record[key]
  return typeof value === 'number' && Number.isFinite(value)
    ? value
    : { reason: `${where}.${key} is ${describe(value)} — expected a finite number` }
}

function optionalString(record: JsonRecord, key: string, where: string): string | undefined | { reason: string } {
  const value = record[key]
  if (value === undefined) return undefined
  return typeof value === 'string' ? value : { reason: `${where}.${key} is ${describe(value)} — expected a string` }
}

function readMatrixRow(element: JsonRecord, at_: string, frameLen: number): ChecksumRow | { reason: string } {
  const kind = element.kind
  if (typeof kind !== 'string' || kind === '') {
    return { reason: `${at_}.kind is ${describe(kind)} — expected a non-empty string` }
  }
  const encoding = element.encoding
  if (typeof encoding !== 'string' || encoding === '') {
    return { reason: `${at_}.encoding is ${describe(encoding)} — expected a non-empty string` }
  }
  if (element.error !== undefined) {
    // Error row: the algorithm never ran, so there is no verdict and normally
    // no `at`. An `at` that is present must still be sane; `ok:true` next to
    // an error is a contradiction, not a formatting choice.
    if (typeof element.error !== 'string' || element.error === '') {
      return { reason: `${at_}.error is ${describe(element.error)} — expected a non-empty reason string` }
    }
    if (element.ok === true) {
      return { reason: `${at_} claims ok:true yet carries error ${JSON.stringify(element.error)} — a row cannot both verify and fail to run` }
    }
    let at: number | undefined
    if (element.at !== undefined) {
      const read = finiteAt(element, 'at', at_)
      if (typeof read === 'object') return read
      if (read < 0 || read >= frameLen) {
        return { reason: `${at_}.at is ${read} — outside the frame (0..${frameLen - 1})` }
      }
      at = read
    }
    return { kind, encoding, at, ok: false, error: element.error }
  }
  const at = finiteAt(element, 'at', at_)
  if (typeof at === 'object') return at
  if (at < 0 || at >= frameLen) {
    return { reason: `${at_}.at is ${at} — outside the frame (0..${frameLen - 1})` }
  }
  if (typeof element.ok !== 'boolean') {
    return { reason: `${at_}.ok is ${describe(element.ok)} — expected a boolean` }
  }
  const optionals: Partial<Record<'expected' | 'actual' | 'computed', string>> = {}
  for (const key of ['expected', 'actual', 'computed'] as const) {
    const read = optionalString(element, key, at_)
    if (typeof read === 'object' && read !== null) return read
    if (read !== undefined) optionals[key] = read
  }
  return {
    kind,
    encoding,
    at,
    ok: element.ok,
    expected: optionals.expected,
    actual: optionals.actual,
    computed: optionals.computed,
  }
}

function readMatrix(value: unknown, frameLen: number): ChecksumRow[] | { reason: string } {
  if (!Array.isArray(value)) {
    return { reason: `checksum_matrix is ${describe(value)} — expected an array` }
  }
  const rows: ChecksumRow[] = []
  for (const [index, element] of value.entries()) {
    const at_ = `checksum_matrix[${index}]`
    if (!isRecord(element)) return { reason: `${at_} is ${describe(element)} — expected an object` }
    const row = readMatrixRow(element, at_, frameLen)
    if ('reason' in row) return row
    rows.push(row)
  }
  return rows
}

function readAttempts(value: unknown): ParseAttempt[] | { reason: string } {
  if (!Array.isArray(value)) {
    return { reason: `parse_attempts is ${describe(value)} — expected an array` }
  }
  const attempts: ParseAttempt[] = []
  for (const [index, element] of value.entries()) {
    const at = `parse_attempts[${index}]`
    if (!isRecord(element)) return { reason: `${at} is ${describe(element)} — expected an object` }
    const offset = finiteAt(element, 'offset', at)
    if (typeof offset === 'object') return offset
    // The server probes fixed shifts around the frame start (−2..=+2), so a
    // negative offset — the parse assuming bytes before the frame — is a
    // legitimate, usually failed, hypothesis. Only non-integers are nonsense.
    if (!Number.isInteger(offset)) {
      return { reason: `${at}.offset is ${offset} — expected an integer byte shift` }
    }
    if (typeof element.parsed !== 'boolean') {
      const legacy = typeof element.ok === 'boolean' ? ' (found a legacy "ok" field — the server predates the parsed rename)' : ''
      return { reason: `${at}.parsed is ${describe(element.parsed)} — expected a boolean${legacy}` }
    }
    if (element.parsed) {
      if (!isRecord(element.fields)) {
        return { reason: `${at} claims parsed but fields is ${describe(element.fields)} — a parsed attempt must carry its decoded fields` }
      }
      attempts.push({ offset, parsed: true, fields: element.fields, error: undefined })
    } else {
      if (typeof element.error !== 'string' || element.error === '') {
        return { reason: `${at} claims failure but error is ${describe(element.error)} — a failed attempt must say why` }
      }
      attempts.push({ offset, parsed: false, fields: undefined, error: element.error })
    }
  }
  return attempts
}

function topString(structured: JsonRecord, key: string): string | undefined {
  const value = structured[key]
  return typeof value === 'string' ? value : undefined
}

/**
 * Reads the diagnostics payload off the full result object. `label` names the
 * declaration source ("view" or "encoding") in reasons; `replay` is computed
 * by dispatch from the result's `port` field.
 */
export function readDiagnostics(structured: JsonRecord, label: string, replay: boolean): DiagnosticsRead {
  const hex = structured.hex
  if (typeof hex !== 'string') {
    return invalid(`${label} declares diagnostics but the result's hex is ${describe(hex)} — expected the frame bytes as a hex string`)
  }
  const bytes = parseHexBytes(hex, 'hex')
  if ('reason' in bytes) return invalid(bytes.reason)
  const frameLen = finiteAt(structured, 'frame_len', 'result')
  if (typeof frameLen === 'object') return invalid(frameLen.reason)
  if (frameLen !== bytes.length) {
    return invalid(`result.frame_len says ${frameLen} but hex holds ${bytes.length} bytes — the payload contradicts itself`)
  }
  const matrix = readMatrix(structured.checksum_matrix, frameLen)
  if ('reason' in matrix) return invalid(matrix.reason)
  const rawAttempts = structured.parse_attempts
  let attempts: readonly ParseAttempt[] | undefined
  if (rawAttempts === undefined) {
    attempts = undefined
  } else {
    const read = readAttempts(rawAttempts)
    if ('reason' in read) return invalid(read.reason)
    attempts = read
  }
  return {
    kind: 'diagnostics',
    diagnostics: {
      bytes,
      frameLen,
      matrix,
      attempts,
      device: topString(structured, 'device'),
      command: topString(structured, 'command'),
      port: topString(structured, 'port'),
      replay,
    },
  }
}

/**
 * Byte footprint of a verified checksum row inside the frame. The contract
 * pins `at` to frame_len − stored_len, so the stored checksum runs from `at`
 * to the end of the frame — the span is exactly what remains. Error rows have
 * no footprint and never reach here (the type says so).
 */
export function checksumSpan(row: ChecksumVerdict, frameLen: number): number {
  return frameLen - row.at
}
