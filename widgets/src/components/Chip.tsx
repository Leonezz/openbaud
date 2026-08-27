import type { ReactNode } from 'react'

export type ChipVariant = 'plain' | 'accent' | 'warn' | 'danger'

export interface ChipProps {
  variant?: ChipVariant
  className?: string
  children: ReactNode
}

const VARIANT_CLASS: Record<ChipVariant, string> = {
  plain: 'ob-chip',
  accent: 'ob-chip ob-chip--accent',
  warn: 'ob-chip ob-chip--warn',
  danger: 'ob-chip ob-chip--danger',
}

export function Chip({ variant = 'plain', className, children }: ChipProps) {
  const base = VARIANT_CLASS[variant]
  return <span className={className ? `${base} ${className}` : base}>{children}</span>
}
