import type { ToolResult } from '../../mcp/useWidget'

/**
 * Serial port candidate — exactly the PortInfo serde shape from
 * crates/openbaud/src/engine/transport.rs (absent optionals are omitted keys).
 * list_ports carries NO profile-match data (verified against
 * crates/openbaud/src/mcp/tools.rs), so the UI never claims one.
 */
export interface PortCandidate {
  readonly path: string
  readonly type: string
  readonly vid?: string
  readonly pid?: string
  readonly manufacturer?: string
  readonly product?: string
  readonly serial_number?: string
}

export const MOCK_ECHO = 'mock:echo'

/** Local stand-in for the "Use mock:echo" action; the real server always
 *  offers this loopback in list_ports, so the shape matches. */
export const MOCK_ECHO_CANDIDATE: PortCandidate = { path: MOCK_ECHO, type: 'mock' }

/** open tool result (tools.rs tool_open): { session_id, port, baud, framing }. */
export interface OpenedSession {
  readonly sessionId: string
  readonly port: string
  readonly baud: number
  readonly framing: string
}

/** Concatenated text blocks of a tool result — server error strings live here. */
export function toolResultText(result: ToolResult): string {
  const content = Array.isArray(result.content) ? result.content : []
  return content
    .filter((block): block is { type: 'text'; text: string } => {
      if (typeof block !== 'object' || block === null) return false
      const candidate = block as { type?: unknown; text?: unknown }
      return candidate.type === 'text' && typeof candidate.text === 'string'
    })
    .map((block) => block.text)
    .join('\n')
}

function optionalString(source: Record<string, unknown>, key: string, where: string): string | undefined {
  const value = source[key]
  if (value === undefined) return undefined
  if (typeof value !== 'string') {
    throw new Error(`list_ports: ${where}.${key} is not a string`)
  }
  return value
}

/** Parse list_ports structuredContent; throws with an explicit reason on any shape mismatch. */
export function parseListPorts(structured: unknown): readonly PortCandidate[] {
  if (typeof structured !== 'object' || structured === null) {
    throw new Error('list_ports: result carries no structuredContent object')
  }
  const ports = (structured as { ports?: unknown }).ports
  if (!Array.isArray(ports)) {
    throw new Error('list_ports: structuredContent.ports is not an array')
  }
  return ports.map((entry, index) => {
    const where = `ports[${index}]`
    if (typeof entry !== 'object' || entry === null) {
      throw new Error(`list_ports: ${where} is not an object`)
    }
    const record = entry as Record<string, unknown>
    if (typeof record.path !== 'string' || typeof record.type !== 'string') {
      throw new Error(`list_ports: ${where} is missing string path/type`)
    }
    return {
      path: record.path,
      type: record.type,
      vid: optionalString(record, 'vid', where),
      pid: optionalString(record, 'pid', where),
      manufacturer: optionalString(record, 'manufacturer', where),
      product: optionalString(record, 'product', where),
      serial_number: optionalString(record, 'serial_number', where),
    }
  })
}

/** Parse the open tool structuredContent; throws with an explicit reason on mismatch. */
export function parseOpenResult(structured: unknown): OpenedSession {
  if (typeof structured !== 'object' || structured === null) {
    throw new Error('open: result carries no structuredContent object')
  }
  const record = structured as Record<string, unknown>
  const { session_id, port, baud, framing } = record
  if (
    typeof session_id !== 'string' ||
    typeof port !== 'string' ||
    typeof baud !== 'number' ||
    typeof framing !== 'string'
  ) {
    throw new Error('open: structuredContent is missing session_id/port/baud/framing')
  }
  return { sessionId: session_id, port, baud, framing }
}

/** Sub-line under the path — real server fields only (type / vid / pid / product). */
export function describePort(port: PortCandidate): string {
  const parts = [port.type.toUpperCase()]
  if (port.vid !== undefined) parts.push(`VID ${port.vid}`)
  if (port.pid !== undefined) parts.push(`PID ${port.pid}`)
  if (port.product !== undefined) parts.push(port.product)
  return parts.join(' · ')
}

/** Device column label (the server provides no profile info to badge with). */
export function deviceLabel(port: PortCandidate): string {
  if (port.type === 'mock') return 'built-in mock'
  return port.manufacturer ?? port.type
}
