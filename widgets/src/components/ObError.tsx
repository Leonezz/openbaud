import type { ReactNode } from 'react'
import { Btn } from './Btn'
import { Icon } from './Icon'

export interface ObErrorProps {
  /** Short statement of what failed (device strings render as text nodes). */
  title: ReactNode
  /** Explicit reason + what to do next — errors are never silent. */
  detail?: ReactNode
  onRetry?: () => void
  retryLabel?: string
}

export function ObError({ title, detail, onRetry, retryLabel = 'Retry' }: ObErrorProps) {
  return (
    <div className="ob-error" role="alert">
      <Icon name="warning" />
      <div style={{ flex: 1 }}>
        <b style={{ display: 'block', marginBottom: 2 }}>{title}</b>
        {detail}
      </div>
      {onRetry && <Btn onClick={onRetry}>{retryLabel}</Btn>}
    </div>
  )
}
