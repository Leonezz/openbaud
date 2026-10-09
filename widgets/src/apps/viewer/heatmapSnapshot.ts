// Validation for a saved result that declares view: { kind: "heatmap" } — a
// command result (run_command, or a parsed frame the agent saved) whose
// parsed[data] holds one grid, row-major. The live branch (a stream
// descriptor) draws a grid per frame; this one draws the grid the result
// carries. Same honesty rule as stream.ts: a grid that does not fit the
// declaration is a named reason, never a silently blank or partial picture.
// Standalone on purpose (no imports) so it runs under `node --test`.

type JsonRecord = Record<string, unknown>

export interface HeatmapSnapshot {
  /** The parsed field the cells come from. */
  readonly data: string
  readonly rows: number
  readonly cols: number
  /** rows*cols finite numbers, row-major. */
  readonly values: readonly number[]
  /** The data field's unit from the result's `units`, when it declares one. */
  readonly unit: string | undefined
  /** "device · command", when the result names them. */
  readonly source: string | undefined
}

export type HeatmapSnapshotRead =
  | { readonly kind: 'heatmap'; readonly heatmap: HeatmapSnapshot }
  | { readonly kind: 'invalid'; readonly reason: string }

function invalid(reason: string): { kind: 'invalid'; reason: string } {
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

function positiveInt(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isInteger(value) && value > 0 ? value : undefined
}

/**
 * Reads a heatmap declaration — the result's own `view`, or an `encoding` the
 * agent passed to show_result; `label` names it in reasons — against
 * `result.parsed`.
 */
export function readHeatmapSnapshot(
  result: JsonRecord,
  view: unknown,
  label: string,
): HeatmapSnapshotRead {
  if (!isRecord(view)) return invalid(`${label} must be an object — got ${describe(view)}`)
  const data = view.data
  if (typeof data !== 'string' || data === '') {
    return invalid(`${label}.data must name the parsed array field — got ${describe(data)}`)
  }
  const rows = positiveInt(view.rows)
  if (rows === undefined) {
    return invalid(`${label}.rows must be a positive integer — got ${describe(view.rows)}`)
  }
  const cols = positiveInt(view.cols)
  if (cols === undefined) {
    return invalid(`${label}.cols must be a positive integer — got ${describe(view.cols)}`)
  }
  const parsed = result.parsed
  if (!isRecord(parsed)) {
    return invalid(
      `${label} declares a heatmap but the result carries no "parsed" object — got ${describe(parsed)}`,
    )
  }
  const cells = parsed[data]
  if (!Array.isArray(cells)) {
    return invalid(`parsed.${JSON.stringify(data)} is not an array — got ${describe(cells)}`)
  }
  const wanted = rows * cols
  if (cells.length !== wanted) {
    return invalid(
      `parsed.${JSON.stringify(data)} holds ${cells.length} values — the heatmap declares ${rows}×${cols} = ${wanted}`,
    )
  }
  const values: number[] = []
  for (const [index, cell] of cells.entries()) {
    if (typeof cell !== 'number' || !Number.isFinite(cell)) {
      return invalid(
        `parsed.${JSON.stringify(data)}[${index}] is a non-numeric cell — got ${describe(cell)}`,
      )
    }
    values.push(cell)
  }
  const units = result.units
  const unitValue = isRecord(units) ? units[data] : undefined
  const unit = typeof unitValue === 'string' ? unitValue : undefined
  const device = typeof result.device === 'string' ? result.device : undefined
  const command = typeof result.command === 'string' ? result.command : undefined
  const source = device !== undefined && command !== undefined ? `${device} · ${command}` : undefined
  return { kind: 'heatmap', heatmap: { data, rows, cols, values, unit, source } }
}
