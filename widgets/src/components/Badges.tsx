import type { ReactNode } from 'react'

// Disclosure badges are essential information (firmware-generated or replayed
// data must always be labelled); never conditionally hide them for polish.

export function BadgeSim({ children = 'SIMULATED SCENE' }: { children?: ReactNode }) {
  return <span className="ob-badge-sim">{children}</span>
}

export function BadgeReplay({ children = 'REPLAY' }: { children?: ReactNode }) {
  return <span className="ob-badge-replay">{children}</span>
}
