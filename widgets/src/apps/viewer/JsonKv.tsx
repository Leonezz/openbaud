import { useState, type ReactNode } from 'react'
import { Kv } from '../../components/Kv'
import { isRecord, truncationMarker } from './dispatch'

// Long arrays are folded to keep the card readable, but folding is this
// widget's own doing and says so — only a `{"truncated": N}` marker means the
// server actually dropped elements (output.rs), and that is reported apart.
const FOLD_ARRAY_AFTER = 8
const MAX_DEPTH = 5

/** Recursive read-only JSON renderer. Every leaf is a plain text node. */
export function JsonKv({ value }: { value: unknown }) {
  return <>{renderValue(value, 0)}</>
}

function renderValue(value: unknown, depth: number): ReactNode {
  if (value === null) return 'null'
  if (typeof value === 'string') return value
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  if (depth >= MAX_DEPTH) return JSON.stringify(value)
  if (Array.isArray(value)) return renderArray(value, depth)
  if (isRecord(value)) return renderRecord(value, depth)
  return String(value)
}

function renderRecord(record: Record<string, unknown>, depth: number): ReactNode {
  const entries = Object.entries(record)
  if (entries.length === 0) return <span className="jsonkv-muted">(empty object)</span>
  return (
    <Kv entries={entries.map(([key, child]) => ({ key, value: renderValue(child, depth + 1) }))} />
  )
}

function renderArray(items: readonly unknown[], depth: number): ReactNode {
  let droppedByServer = 0
  const present: unknown[] = []
  for (const item of items) {
    const marker = truncationMarker(item)
    if (marker !== undefined) droppedByServer += marker
    else present.push(item)
  }
  if (present.length === 0 && droppedByServer === 0) return '[]'
  return <ArrayValue items={present} droppedByServer={droppedByServer} depth={depth} />
}

function ArrayValue({
  items,
  droppedByServer,
  depth,
}: {
  items: readonly unknown[]
  droppedByServer: number
  depth: number
}) {
  const [expanded, setExpanded] = useState(false)
  const folded = !expanded && items.length > FOLD_ARRAY_AFTER
  const shown = folded ? items.slice(0, FOLD_ARRAY_AFTER) : items
  return (
    <div style={{ display: 'grid', gap: 2 }}>
      {shown.map((item, index) => (
        <div key={index} style={{ display: 'flex', gap: 8 }}>
          <span className="jsonkv-index">[{index}]</span>
          <div style={{ flex: 1 }}>{renderValue(item, depth + 1)}</div>
        </div>
      ))}
      {folded && (
        <button type="button" className="jsonkv-more" onClick={() => setExpanded(true)}>
          show all {items.length}
        </button>
      )}
      {expanded && items.length > FOLD_ARRAY_AFTER && (
        <button type="button" className="jsonkv-more" onClick={() => setExpanded(false)}>
          show fewer
        </button>
      )}
      {droppedByServer > 0 && (
        <span className="jsonkv-muted">
          {droppedByServer} more dropped from this summary — the complete array is in the
          full_result file below
        </span>
      )}
    </div>
  )
}
