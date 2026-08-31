// Pure canvas painter for the live heatmap surface. Same discipline as the
// rest of src/render: no React, no MCP — (ctx, geometry, data, cssVars) in,
// pixels out. The color ramp interpolates between two *theme tokens* read at
// paint time (--scope-bg → --s1), so light/dark each get their own readable
// ramp and a host theme switch only needs a repaint.

export type CssVarReader = (name: string) => string

export interface Rgb {
  readonly r: number
  readonly g: number
  readonly b: number
}

/**
 * Parses the color forms our tokens actually use: #rgb, #rrggbb, rgb()/rgba().
 * Anything else is a setup defect and throws — never silently paints black.
 */
export function parseCssColor(value: string): Rgb {
  const text = value.trim()
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(text)
  if (hex !== null && hex[1] !== undefined) {
    const digits = hex[1]
    const wide = digits.length === 6 ? digits : [...digits].map((d) => d + d).join('')
    return {
      r: Number.parseInt(wide.slice(0, 2), 16),
      g: Number.parseInt(wide.slice(2, 4), 16),
      b: Number.parseInt(wide.slice(4, 6), 16),
    }
  }
  const fn = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/.exec(text)
  if (fn !== null && fn[1] !== undefined && fn[2] !== undefined && fn[3] !== undefined) {
    return { r: Number(fn[1]), g: Number(fn[2]), b: Number(fn[3]) }
  }
  throw new Error(`heat: cannot parse color ${JSON.stringify(value)} — expected hex or rgb()/rgba()`)
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t
}

/** Linear ramp between two parsed colors; t is clamped to [0, 1]. */
export function rampColor(lo: Rgb, hi: Rgb, t: number): string {
  const k = Math.min(1, Math.max(0, t))
  return `rgb(${Math.round(lerp(lo.r, hi.r, k))}, ${Math.round(lerp(lo.g, hi.g, k))}, ${Math.round(lerp(lo.b, hi.b, k))})`
}

export interface GridRange {
  readonly min: number
  readonly max: number
}

/** Measured min/max of one grid — the legend and the cells must agree. */
export function gridRange(values: readonly number[]): GridRange {
  let min = Infinity
  let max = -Infinity
  for (const v of values) {
    if (v < min) min = v
    if (v > max) max = v
  }
  return { min, max }
}

export interface HeatmapGeometry {
  readonly rows: number
  readonly cols: number
  /** Canvas device pixels. */
  readonly width: number
  readonly height: number
}

/**
 * Paints `values` (row-major, rows*cols entries) into the canvas. A flat range
 * (min === max) paints every cell at the low end — a uniform field carries no
 * contrast and pretending otherwise would invent data. A 1-device-pixel seam
 * between cells comes from the surface underneath (the ctx is cleared first).
 */
export function drawHeatmapGrid(
  ctx: CanvasRenderingContext2D,
  geometry: HeatmapGeometry,
  values: readonly number[],
  cssVars: CssVarReader,
): void {
  const { rows, cols, width, height } = geometry
  const lo = parseCssColor(cssVars('--scope-bg'))
  const hi = parseCssColor(cssVars('--s1'))
  const { min, max } = gridRange(values)
  const span = max - min
  ctx.clearRect(0, 0, width, height)
  const cellW = width / cols
  const cellH = height / rows
  const seam = Math.max(1, Math.min(cellW, cellH) * 0.04)
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < cols; col += 1) {
      const value = values[row * cols + col]
      if (value === undefined) continue
      const t = span > 0 ? (value - min) / span : 0
      ctx.fillStyle = rampColor(lo, hi, t)
      ctx.fillRect(col * cellW, row * cellH, cellW - seam, cellH - seam)
    }
  }
}
