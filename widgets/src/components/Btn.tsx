import type { MouseEventHandler, ReactNode } from 'react'

export type BtnVariant = 'default' | 'primary' | 'ghost' | 'arm'

export interface BtnProps {
  variant?: BtnVariant
  disabled?: boolean
  onClick?: MouseEventHandler<HTMLButtonElement>
  title?: string
  'aria-label'?: string
  className?: string
  children: ReactNode
}

const VARIANT_CLASS: Record<BtnVariant, string> = {
  default: 'ob-btn',
  primary: 'ob-btn ob-btn--primary',
  ghost: 'ob-btn ob-btn--ghost',
  arm: 'ob-btn ob-btn--arm',
}

export function Btn({
  variant = 'default',
  disabled = false,
  onClick,
  title,
  'aria-label': ariaLabel,
  className,
  children,
}: BtnProps) {
  const base = VARIANT_CLASS[variant]
  return (
    <button
      type="button"
      className={className ? `${base} ${className}` : base}
      disabled={disabled}
      onClick={onClick}
      title={title}
      aria-label={ariaLabel}
    >
      {children}
    </button>
  )
}
