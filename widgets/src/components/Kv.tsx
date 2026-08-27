import type { ReactNode } from 'react'

export interface KvEntry {
  key: string
  value: ReactNode
}

export interface KvProps {
  entries: readonly KvEntry[]
}

export function Kv({ entries }: KvProps) {
  return (
    <dl className="ob-kv">
      {entries.map((entry) => (
        <div key={entry.key} style={{ display: 'contents' }}>
          <dt>{entry.key}</dt>
          <dd>{entry.value}</dd>
        </div>
      ))}
    </dl>
  )
}
