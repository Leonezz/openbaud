import type { ToolResult } from '../../mcp/useWidget'

/**
 * ask_port candidate: the PortInfo serde shape from
 * crates/openbaud/src/engine/transport.rs plus the workspace enrichment the
 * ask_port tool adds. Absent optionals are omitted keys, so every enrichment
 * field is optional and the same parser accepts a plain list_ports entry.
 */
export interface EnrichedPort {
  readonly path: string
  readonly type: string
  readonly vid?: string
  readonly pid?: string
  readonly manufacturer?: string
  readonly product?: string
  readonly serial_number?: string
  /** Workspace devices whose selector matches this port — a real profile match. */
  readonly matches_devices?: readonly string[]
  /** Session id already holding this port; such a row cannot be picked. */
  readonly open_session?: string
  /** Canonical path this node duplicates (macOS /dev/tty.X twins /dev/cu.X). */
  readonly alias_of?: string
}

/** ask_port structuredContent: why the agent asked, for which device, and the candidates. */
export interface AskPort {
  readonly reason?: string
  readonly device?: string
  readonly candidates: readonly EnrichedPort[]
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
  if (value === undefined || value === null) return undefined
  if (typeof value !== 'string') {
    throw new Error(`${where}.${key} is not a string`)
  }
  return value
}

function optionalStringArray(
  source: Record<string, unknown>,
  key: string,
  where: string,
): readonly string[] | undefined {
  const value = source[key]
  if (value === undefined || value === null) return undefined
  if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) {
    throw new Error(`${where}.${key} is not an array of strings`)
  }
  return value as readonly string[]
}

function parsePort(entry: unknown, where: string): EnrichedPort {
  if (typeof entry !== 'object' || entry === null) {
    throw new Error(`${where} is not an object`)
  }
  const record = entry as Record<string, unknown>
  if (typeof record.path !== 'string' || typeof record.type !== 'string') {
    throw new Error(`${where} is missing string path/type`)
  }
  return {
    path: record.path,
    type: record.type,
    vid: optionalString(record, 'vid', where),
    pid: optionalString(record, 'pid', where),
    manufacturer: optionalString(record, 'manufacturer', where),
    product: optionalString(record, 'product', where),
    serial_number: optionalString(record, 'serial_number', where),
    matches_devices: optionalStringArray(record, 'matches_devices', where),
    open_session: optionalString(record, 'open_session', where),
    alias_of: optionalString(record, 'alias_of', where),
  }
}

function parsePorts(value: unknown, where: string): readonly EnrichedPort[] {
  if (!Array.isArray(value)) {
    throw new Error(`${where} is not an array`)
  }
  return value.map((entry, index) => parsePort(entry, `${where}[${index}]`))
}

/** Parse ask_port structuredContent; throws with an explicit reason on any shape mismatch. */
export function parseAskPort(structured: unknown): AskPort {
  if (typeof structured !== 'object' || structured === null) {
    throw new Error('ask_port: result carries no structuredContent object')
  }
  const record = structured as Record<string, unknown>
  return {
    reason: optionalString(record, 'reason', 'ask_port: structuredContent'),
    device: optionalString(record, 'device', 'ask_port: structuredContent'),
    candidates: parsePorts(record.candidates, 'ask_port: structuredContent.candidates'),
  }
}

/** Parse list_ports structuredContent (the Rescan path); same entry shape. */
export function parseListPorts(structured: unknown): readonly EnrichedPort[] {
  if (typeof structured !== 'object' || structured === null) {
    throw new Error('list_ports: result carries no structuredContent object')
  }
  return parsePorts((structured as { ports?: unknown }).ports, 'list_ports: structuredContent.ports')
}

/** A port held by another session cannot be handed to the agent. */
export function isBlocked(port: EnrichedPort): boolean {
  return port.open_session !== undefined
}

/** canonical path → alias paths pointing at it (macOS tty twins). */
export function aliasesByCanonical(
  candidates: readonly EnrichedPort[],
): ReadonlyMap<string, readonly string[]> {
  return candidates.reduce((acc, port) => {
    const canonical = port.alias_of
    if (canonical === undefined) return acc
    return new Map(acc).set(canonical, [...(acc.get(canonical) ?? []), port.path])
  }, new Map<string, readonly string[]>())
}

/** Alias entries are folded away by default; the head offers the toggle. */
export function visiblePorts(
  candidates: readonly EnrichedPort[],
  showAliases: boolean,
): readonly EnrichedPort[] {
  return showAliases ? candidates : candidates.filter((port) => port.alias_of === undefined)
}

/**
 * First row the picker preselects. When ask_port was invoked for a specific
 * workspace device (the tool input's `device`, echoed back in the result),
 * the first visible unblocked row whose `matches_devices` names that device
 * wins — the agent asked on that device's behalf, so its matching port is the
 * likeliest pick. Without a device, or when no unblocked row matches it, the
 * previous rule stands unchanged: the first visible unblocked candidate.
 * Preselection only — every row stays clickable either way.
 */
export function firstSelectable(
  ports: readonly EnrichedPort[],
  device?: string,
): string | null {
  if (device !== undefined) {
    const matched = ports.find(
      (port) => !isBlocked(port) && (port.matches_devices?.includes(device) ?? false),
    )
    if (matched !== undefined) return matched.path
  }
  return ports.find((port) => !isBlocked(port))?.path ?? null
}

export function findPort(
  candidates: readonly EnrichedPort[],
  path: string | null,
): EnrichedPort | undefined {
  return path === null ? undefined : candidates.find((port) => port.path === path)
}

/** Sub-line under the path — server fields only (type / vid / pid / manufacturer / product). */
export function describePort(port: EnrichedPort): string {
  const parts = [port.type.toUpperCase()]
  if (port.vid !== undefined) parts.push(`VID ${port.vid}`)
  if (port.pid !== undefined) parts.push(`PID ${port.pid}`)
  if (port.manufacturer !== undefined) parts.push(port.manufacturer)
  if (port.product !== undefined) parts.push(port.product)
  return parts.join(' · ')
}

/**
 * Structured payload pushed to the agent via ui/update-model-context.
 * Extends Record so it satisfies the SDK's structuredContent parameter.
 */
export interface PortSelection extends Record<string, unknown> {
  readonly kind: 'port_selection'
  readonly path: string
  readonly matches_devices?: readonly string[]
  readonly reason?: string
}

export function portSelection(port: EnrichedPort, reason: string | undefined): PortSelection {
  const matches = port.matches_devices
  return {
    kind: 'port_selection',
    path: port.path,
    ...(matches !== undefined && matches.length > 0 ? { matches_devices: matches } : {}),
    ...(reason !== undefined ? { reason } : {}),
  }
}

/** Text block twin of the payload — the agent, not the widget, opens the port. */
export function selectionText(port: EnrichedPort, device: string | undefined): string {
  const target = device !== undefined ? ` for device ${device}` : ''
  return `User picked serial port ${port.path}${target}. Nothing was opened by the picker.`
}
