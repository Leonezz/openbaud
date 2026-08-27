import type { ReactElement } from 'react'

export type IconName =
  | 'play'
  | 'play-solid'
  | 'pause'
  | 'prev'
  | 'next'
  | 'expand'
  | 'warning'
  | 'warning-alt'
  | 'target'

// Paths are verbatim from docs/design/mcp-apps-ui/library/ cards (icon
// discipline: inline SVG, stroke currentColor, never emoji codepoints).
// 'warning' = feedback-states/chips-badges triangle; 'warning-alt' = the
// w04 danger-confirm variant. The design renders Retry as a text-only
// button, so no retry glyph exists in the SoT.
const GLYPHS: Record<IconName, ReactElement> = {
  play: <path d="M6 4.5v15l13-7.5Z" />,
  'play-solid': <path d="M8 5v14l11-7z" fill="currentColor" stroke="none" />,
  pause: <path d="M9 5v14M15 5v14" />,
  prev: (
    <>
      <path d="M18 5v14l-9-7z" fill="currentColor" stroke="none" />
      <path d="M7 5v14" />
    </>
  ),
  next: (
    <>
      <path d="M6 5v14l9-7z" fill="currentColor" stroke="none" />
      <path d="M17 5v14" />
    </>
  ),
  expand: <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />,
  warning: (
    <>
      <path d="M12 3 2.5 20h19Z" />
      <line x1="12" y1="10" x2="12" y2="14" />
      <line x1="12" y1="17" x2="12" y2="17.01" />
    </>
  ),
  'warning-alt': (
    <>
      <path d="M12 4 2.5 20h19z" />
      <path d="M12 10v4.5" />
      <circle cx="12" cy="17.2" r=".7" fill="currentColor" stroke="none" />
    </>
  ),
  target: (
    <>
      <circle cx="12" cy="12" r="7" />
      <circle cx="12" cy="12" r="2.5" />
    </>
  ),
}

export interface IconProps {
  name: IconName
  className?: string
}

export function Icon({ name, className }: IconProps) {
  return (
    <svg
      className={className ? `ob-icon ${className}` : 'ob-icon'}
      viewBox="0 0 24 24"
      aria-hidden="true"
    >
      {GLYPHS[name]}
    </svg>
  )
}
