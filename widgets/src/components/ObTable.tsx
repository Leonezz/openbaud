import type { ReactNode } from 'react'

export interface ObTableProps {
  /** Column headers, rendered as th in order. */
  head: readonly ReactNode[]
  className?: string
  /** aria semantics (e.g. role="radiogroup" for single-select pickers). */
  role?: string
  'aria-label'?: string
  /** tbody rows — use ObTableRow. */
  children: ReactNode
}

export function ObTable({
  head,
  className,
  role,
  'aria-label': ariaLabel,
  children,
}: ObTableProps) {
  return (
    <table
      className={className ? `ob-table ${className}` : 'ob-table'}
      role={role}
      aria-label={ariaLabel}
    >
      <thead>
        <tr>
          {head.map((h, i) => (
            <th key={i}>{h}</th>
          ))}
        </tr>
      </thead>
      <tbody>{children}</tbody>
    </table>
  )
}

export interface ObTableRowProps {
  selectable?: boolean
  selected?: boolean
  onSelect?: () => void
  role?: string
  children: ReactNode
}

export function ObTableRow({ selectable, selected, onSelect, role, children }: ObTableRowProps) {
  const cls =
    [selectable ? 'is-selectable' : '', selected ? 'is-selected' : '']
      .filter(Boolean)
      .join(' ') || undefined
  return (
    <tr
      className={cls}
      role={role}
      aria-checked={role === 'radio' ? selected === true : undefined}
      tabIndex={selectable ? 0 : undefined}
      onClick={selectable ? onSelect : undefined}
      onKeyDown={
        selectable
          ? (e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                onSelect?.()
              }
            }
          : undefined
      }
    >
      {children}
    </tr>
  )
}
