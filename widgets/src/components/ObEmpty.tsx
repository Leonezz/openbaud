import type { ReactNode } from 'react'

export function ObEmpty({ children }: { children: ReactNode }) {
  return <div className="ob-empty">{children}</div>
}
