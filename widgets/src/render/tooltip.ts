// Shared floating tooltip for the chart surfaces (uPlot density plots and the
// polar scope). Same discipline as the rest of src/render: the layout math
// (placeTooltip) is a pure function, the DOM host around it is thin, and no
// React or MCP imports appear here.
//
// Two hard rules live in this file:
// - Content is written exclusively through textContent. Tooltip rows carry
//   device-sourced strings and numbers, which are data, never markup.
// - Colors come from theme tokens via the .ob-tooltip CSS block (viewer.css),
//   so a host theme switch restyles the tooltip with zero JS involved.
import type uPlot from 'uplot'

export interface TooltipLine {
  readonly label: string
  readonly value: string
}

export interface TooltipPlacement {
  readonly left: number
  readonly top: number
}

/**
 * Pure placement: prefer the right of the anchor, flip to the left when the
 * tip would overflow the host; vertically centered on the anchor and clamped
 * into the host box. All values are CSS pixels in the host's own space.
 */
export function placeTooltip(
  hostW: number,
  hostH: number,
  tipW: number,
  tipH: number,
  anchorX: number,
  anchorY: number,
  gap = 12,
): TooltipPlacement {
  const left = anchorX + gap + tipW <= hostW ? anchorX + gap : Math.max(0, anchorX - gap - tipW)
  const top = Math.min(Math.max(0, anchorY - tipH / 2), Math.max(0, hostH - tipH))
  return { left, top }
}

export interface TooltipHost {
  /** Render `lines` and place the tip near the anchor (host CSS pixels). */
  show(anchorX: number, anchorY: number, lines: readonly TooltipLine[]): void
  hide(): void
  destroy(): void
}

function renderLine(line: TooltipLine): HTMLDivElement {
  const row = document.createElement('div')
  row.className = 'ob-tooltip__row'
  const label = document.createElement('span')
  label.className = 'ob-tooltip__label'
  label.textContent = line.label
  const value = document.createElement('span')
  value.className = 'ob-tooltip__value'
  value.textContent = line.value
  row.append(label, value)
  return row
}

/**
 * Mounts one hidden .ob-tooltip div into `host`, which must be a positioned
 * element (the plot container). pointer-events stays none via CSS, so the tip
 * can never steal clicks from the chart gestures underneath it.
 */
export function createTooltipHost(host: HTMLElement): TooltipHost {
  const el = document.createElement('div')
  el.className = 'ob-tooltip'
  el.style.display = 'none'
  host.appendChild(el)
  const hide = (): void => {
    el.style.display = 'none'
  }
  return {
    show(anchorX: number, anchorY: number, lines: readonly TooltipLine[]): void {
      if (lines.length === 0) {
        // Nothing real to say at this position — an empty box would only
        // pretend there is data here.
        hide()
        return
      }
      el.replaceChildren(...lines.map(renderLine))
      // Must be visible before measuring, or offsetWidth/Height read 0.
      el.style.display = 'block'
      const spot = placeTooltip(
        host.clientWidth,
        host.clientHeight,
        el.offsetWidth,
        el.offsetHeight,
        anchorX,
        anchorY,
      )
      el.style.left = `${spot.left}px`
      el.style.top = `${spot.top}px`
    },
    hide,
    destroy(): void {
      el.remove()
    },
  }
}

export interface UplotCursor {
  /** CSS pixels relative to u.over (uPlot's own cursor space). */
  readonly cssX: number
  readonly cssY: number
  /** Canvas device pixels — the space u.bbox and the overlay painters use. */
  readonly devX: number
  readonly devY: number
}

/** Current uPlot cursor in both coordinate spaces, or null when off-plot. */
export function readUplotCursor(u: uPlot, pxRatio: number): UplotCursor | null {
  const { left, top } = u.cursor
  if (left === undefined || top === undefined || left < 0 || top < 0) return null
  return {
    cssX: left,
    cssY: top,
    devX: u.bbox.left + left * pxRatio,
    devY: u.bbox.top + top * pxRatio,
  }
}

/**
 * Translates a u.over-relative cursor position into `host`'s coordinate space
 * (the tooltip's offset parent, i.e. the element passed to createTooltipHost).
 */
export function anchorInHost(
  u: uPlot,
  host: HTMLElement,
  cssX: number,
  cssY: number,
): { readonly x: number; readonly y: number } {
  const overBox = u.over.getBoundingClientRect()
  const hostBox = host.getBoundingClientRect()
  return { x: overBox.left - hostBox.left + cssX, y: overBox.top - hostBox.top + cssY }
}
