import type { ReactNode } from 'react'

export interface CardProps {
  /** Card title rendered as .ob-card__title; device-sourced text stays a text node. */
  title?: ReactNode
  /** Extra head content after the title (badges, spacer, LED, buttons). */
  head?: ReactNode
  /** Foot slot; rendered inside .ob-card__foot when present. */
  foot?: ReactNode
  /** Wrap children in the padded .ob-card__body (off for full-bleed tables). */
  pad?: boolean
  className?: string
  children?: ReactNode
}

export function Card({ title, head, foot, pad = true, className, children }: CardProps) {
  return (
    <div className={className ? `ob-card ${className}` : 'ob-card'}>
      {(title !== undefined || head !== undefined) && (
        <header className="ob-card__head">
          {title !== undefined && <span className="ob-card__title">{title}</span>}
          {head}
        </header>
      )}
      {pad ? <div className="ob-card__body">{children}</div> : children}
      {foot !== undefined && <footer className="ob-card__foot">{foot}</footer>}
    </div>
  )
}

/** Flexible gap used inside head/foot rows (.ob-card__spacer). */
export function CardSpacer() {
  return <span className="ob-card__spacer" />
}
