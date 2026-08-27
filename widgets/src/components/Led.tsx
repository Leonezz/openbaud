export type LedTone = 'ok' | 'warn' | 'err' | 'off'

export interface LedProps {
  tone: LedTone
  /** Never color-alone: always pair the LED with adjacent status text. */
  pulse?: boolean
}

export function Led({ tone, pulse = false }: LedProps) {
  const cls = `ob-led ob-led--${tone}${pulse ? ' ob-led--pulse' : ''}`
  return <span className={cls} />
}
