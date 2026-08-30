// Thin lifecycle helper around one uPlot instance: create, resize, destroy.
// Two project conventions are enforced here rather than re-invented per view:
//
// 1. Every color handed to uPlot must be a *function* that reads a CSS custom
//    property at draw time (uPlot re-invokes stroke/fill/axis color functions
//    on every draw pass — verified in dist/uPlot.esm.js cacheStrokeFill /
//    drawAxesGrid). A host theme switch therefore needs nothing but redraw():
//    the same functions return the new theme's values.
// 2. Overlay painters (src/render/overlays.ts) mount on uPlot's `draw` hook,
//    which fires after axes and series, and paint onto the same canvas.
//
// No React, no MCP: a DOM element in, a handle out — reusable by any future
// local panel exactly like src/render/polar.ts.
import uPlot from 'uplot'
import 'uplot/dist/uPlot.min.css'

export type CssVarReader = (name: string) => string

/**
 * Token reader bound to `el`: resolves a CSS variable at call time, so the
 * value always belongs to the currently applied theme. A missing token is a
 * setup defect and throws — never silently paints in a default color.
 */
export function cssVarReader(el: Element): CssVarReader {
  return (name: string): string => {
    const value = getComputedStyle(el).getPropertyValue(name).trim()
    if (value === '') {
      throw new Error(`uplot-host: token ${name} is not defined — import src/theme/index.css`)
    }
    return value
  }
}

/** A uPlot stroke/fill function reading one theme token per invocation. */
export function tokenColor(read: CssVarReader, name: string): () => string {
  return () => read(name)
}

export interface UplotHostConfig {
  readonly target: HTMLElement
  readonly opts: uPlot.Options
  readonly data: uPlot.AlignedData
  /** Overlay painter, mounted on the draw hook (fires after axes + series). */
  readonly onDraw?: (u: uPlot) => void
}

export interface UplotHost {
  readonly u: uPlot
  /** Full repaint — re-runs every color function, so theme switch = redraw. */
  redraw(): void
  setSize(width: number, height: number): void
  destroy(): void
}

export function createUplotHost({ target, opts, data, onDraw }: UplotHostConfig): UplotHost {
  // Merge the overlay painter into any hooks the caller already declared —
  // build a new options object, never mutate the caller's.
  const merged: uPlot.Options =
    onDraw === undefined
      ? opts
      : { ...opts, hooks: { ...opts.hooks, draw: [...(opts.hooks?.draw ?? []), onDraw] } }
  const u = new uPlot(merged, data, target)
  return {
    u,
    redraw(): void {
      // uPlot defers its first commit to a microtask (dist/uPlot.esm.js
      // commit()/microTask). A synchronous redraw() before that commit — e.g.
      // from a React effect that runs on mount — paints with unconverged axis
      // sizes and permanently suppresses the initial paint. Until status flips
      // to 1 (end of the first _commit) the pending commit will paint the
      // current state anyway, so skipping here loses nothing.
      if (u.status === 1) u.redraw()
    },
    setSize(width: number, height: number): void {
      u.setSize({ width, height })
    },
    destroy(): void {
      u.destroy()
    },
  }
}

/**
 * Horizontal panning for a zoomed x scale. uPlot ships drag-select zoom and
 * double-click reset built in, but no pan gesture — this adds one: wheel /
 * trackpad scroll over the plot shifts the visible window, clamped to
 * [fullFromMs, fullToMs]. At full zoom-out the wheel is left alone so page
 * scrolling still works. Returns the detach function.
 */
export function attachWheelPan(u: uPlot, fullFromMs: number, fullToMs: number): () => void {
  const onWheel = (event: WheelEvent): void => {
    const { min, max } = u.scales.x ?? {}
    if (min === undefined || max === undefined || min === null || max === null) return
    const visible = max - min
    if (visible >= fullToMs - fullFromMs) return
    event.preventDefault()
    const delta = event.deltaX !== 0 ? event.deltaX : event.deltaY
    const shift = (delta / Math.max(1, u.over.clientWidth)) * visible
    const shifted = { min: min + shift, max: max + shift }
    const clamped =
      shifted.min < fullFromMs
        ? { min: fullFromMs, max: fullFromMs + visible }
        : shifted.max > fullToMs
          ? { min: fullToMs - visible, max: fullToMs }
          : shifted
    u.setScale('x', clamped)
  }
  u.over.addEventListener('wheel', onWheel, { passive: false })
  return () => u.over.removeEventListener('wheel', onWheel)
}
