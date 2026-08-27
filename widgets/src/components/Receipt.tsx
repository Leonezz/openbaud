import type { ReactNode } from 'react'

/** Context receipt: confirms a selection was written to model context. */
export function Receipt({ children }: { children: ReactNode }) {
  return <span className="ob-receipt">{children}</span>
}
