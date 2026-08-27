import type { ReactNode } from 'react'
import { Kv } from '../../components/Kv'
import { isRecord, truncationMarker } from './dispatch'

// Arrays cap at 8 shown entries — the server summary keeps the same head size
// (output.rs ARRAY_SUMMARY_HEAD), so client and server truncation read alike.
const MAX_ARRAY_ITEMS = 8
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
  let serverTruncated = 0
  const real: unknown[] = []
  for (const item of items) {
    const marker = truncationMarker(item)
    if (marker !== undefined) serverTruncated += marker
    else real.push(item)
  }
  const shown = real.slice(0, MAX_ARRAY_ITEMS)
  const hidden = real.length - shown.length + serverTruncated
  if (shown.length === 0 && hidden === 0) return '[]'
  return (
    <div style={{ display: 'grid', gap: 2 }}>
      {shown.map((item, index) => (
        <div key={index} style={{ display: 'flex', gap: 8 }}>
          <span className="jsonkv-index">[{index}]</span>
          <div style={{ flex: 1 }}>{renderValue(item, depth + 1)}</div>
        </div>
      ))}
      {hidden > 0 && <span className="jsonkv-muted">… {hidden} more (truncated)</span>}
    </div>
  )
}
