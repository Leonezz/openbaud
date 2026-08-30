// Pure canvas overlay painters for the session-timeline surface (design SoT:
// docs/design/mcp-apps-ui/library/widgets/w11-session-timeline.html).
// Portable core with zero React and zero MCP imports: every painter is a plain
// (ctx, rect, scales, data, cssVars) function, so the same code serves the MCP
// widget today and any future local panel unchanged. All colors resolve
// through the cssVars reader at call time — a theme switch only needs the
// caller to repaint.
//
// Coordinates are canvas device pixels (uPlot draws its canvas at
// devicePixelRatio); `scales.pxRatio` scales the fixed lane offsets and fonts.

export type CssVarReader = (name: string) => string

/** Plot-area rectangle in canvas device pixels (uPlot's u.bbox shape). */
export interface OverlayRect {
  readonly left: number
  readonly top: number
  readonly width: number
  readonly height: number
}

export interface TimeScales {
  /** Visible window (after zoom/pan), epoch ms. */
  readonly fromMs: number
  readonly toMs: number
  readonly bucketMs: number
  readonly pxRatio: number
}

export interface DensityBucket {
  readonly t0: number
  readonly txBytes: number
  readonly rxBytes: number
}

export type EventMarkerKind = 'cmd' | 'write' | 'deny' | 'workflow' | 'workflow_step'

export interface EventMarker {
  readonly tsMs: number
  readonly kind: EventMarkerKind
  /** Short label drawn beside the shape — kind is never color-alone. */
  readonly label: string
  readonly ok: boolean
}

/** Linear time-to-x mapping — matches uPlot's linear x scale exactly. */
export function timeToPx(rect: OverlayRect, scales: TimeScales, ms: number): number {
  const span = scales.toMs - scales.fromMs
  if (span <= 0) return rect.left
  return rect.left + ((ms - scales.fromMs) / span) * rect.width
}

function monoFont(px: number, weight = ''): string {
  const w = weight === '' ? '' : `${weight} `
  return `${w}${px}px ui-monospace, "SF Mono", Menlo, monospace`
}

function clipped(ctx: CanvasRenderingContext2D, rect: OverlayRect, paint: () => void): void {
  ctx.save()
  ctx.beginPath()
  ctx.rect(rect.left, rect.top, rect.width, rect.height)
  ctx.clip()
  paint()
  ctx.restore()
}

/** Vertical layout of the surface, all offsets relative to rect.top. */
function lanes(rect: OverlayRect, p: number) {
  const densityTop = rect.top + 72 * p
  const densityBottom = rect.top + rect.height - 6 * p
  return {
    gapTextY: rect.top + 12 * p,
    labelRowA: rect.top + 30 * p,
    markerY: rect.top + 44 * p,
    labelRowB: rect.top + 60 * p,
    densityTop,
    midY: (densityTop + densityBottom) / 2,
    amp: Math.max(4 * p, (densityBottom - densityTop) / 2 - 2 * p),
  }
}

/**
 * Bidirectional TX/RX density band: TX bars grow up from the midline (--s2),
 * RX bars grow down (--s1). Bars are normalized against the peak bucket of the
 * whole dataset — not just the visible window — so zooming never rescales the
 * bars under the reader. The caller states the peak as a text KPI.
 */
export function drawDensityTracks(
  ctx: CanvasRenderingContext2D,
  rect: OverlayRect,
  scales: TimeScales,
  buckets: readonly DensityBucket[],
  cssVars: CssVarReader,
): void {
  const p = scales.pxRatio
  const { midY, amp } = lanes(rect, p)
  clipped(ctx, rect, () => {
    ctx.strokeStyle = cssVars('--scope-faint')
    ctx.lineWidth = 1 * p
    ctx.beginPath()
    ctx.moveTo(rect.left, midY)
    ctx.lineTo(rect.left + rect.width, midY)
    ctx.stroke()

    const peak = buckets.reduce((acc, b) => Math.max(acc, b.txBytes, b.rxBytes), 0)
    if (peak <= 0) return

    for (const bucket of buckets) {
      const x0 = timeToPx(rect, scales, bucket.t0)
      const x1 = timeToPx(rect, scales, bucket.t0 + scales.bucketMs)
      if (x1 < rect.left || x0 > rect.left + rect.width) continue
      const w = Math.max(1 * p, x1 - x0 - 1 * p)
      if (bucket.txBytes > 0) {
        const h = Math.max(1.5 * p, (bucket.txBytes / peak) * amp)
        ctx.fillStyle = cssVars('--s2')
        ctx.fillRect(x0, midY - h, w, h)
      }
      if (bucket.rxBytes > 0) {
        const h = Math.max(1.5 * p, (bucket.rxBytes / peak) * amp)
        ctx.fillStyle = cssVars('--s1')
        ctx.fillRect(x0, midY, w, h)
      }
    }
  })
}

/**
 * Bucket covering time `ms`, or null. Density is sparse — a position with no
 * bucket has no data, and must never be reported as a zero-byte bucket.
 */
export function densityBucketAt(
  buckets: readonly DensityBucket[],
  bucketMs: number,
  ms: number,
): DensityBucket | null {
  for (const bucket of buckets) {
    if (ms >= bucket.t0 && ms < bucket.t0 + bucketMs) return bucket
  }
  return null
}

/**
 * Marker-lane geometry for one event timestamp, in canvas device pixels.
 * drawEventMarkers paints through this and hit-testing measures through it,
 * so a tooltip can only open exactly where the shape was drawn.
 */
export interface MarkerSpot {
  readonly x: number
  readonly y: number
  readonly r: number
}

export function eventMarkerSpot(rect: OverlayRect, scales: TimeScales, tsMs: number): MarkerSpot {
  const p = scales.pxRatio
  return { x: timeToPx(rect, scales, tsMs), y: lanes(rect, p).markerY, r: 4.5 * p }
}

/**
 * Index of the event marker under (x, y) — canvas device pixels, the same
 * space the painters draw in — or null when nothing is hit. A small pad
 * widens the target; when markers overlap, the nearest-x one wins.
 */
export function hitEventMarker(
  rect: OverlayRect,
  scales: TimeScales,
  events: readonly EventMarker[],
  x: number,
  y: number,
): number | null {
  const pad = 2.5 * scales.pxRatio
  let bestIndex: number | null = null
  let bestDx = Infinity
  for (const [index, event] of events.entries()) {
    const spot = eventMarkerSpot(rect, scales, event.tsMs)
    const dx = Math.abs(x - spot.x)
    if (dx <= spot.r + pad && Math.abs(y - spot.y) <= spot.r + pad && dx < bestDx) {
      bestIndex = index
      bestDx = dx
    }
  }
  return bestIndex
}

function diamond(ctx: CanvasRenderingContext2D, x: number, y: number, r: number): void {
  ctx.beginPath()
  ctx.moveTo(x, y - r)
  ctx.lineTo(x + r, y)
  ctx.lineTo(x, y + r)
  ctx.lineTo(x - r, y)
  ctx.closePath()
}

function warnTriangle(ctx: CanvasRenderingContext2D, x: number, y: number, r: number): void {
  ctx.beginPath()
  ctx.moveTo(x, y - r)
  ctx.lineTo(x + r * 1.1, y + r * 0.9)
  ctx.lineTo(x - r * 1.1, y + r * 0.9)
  ctx.closePath()
}

/**
 * Event markers on the top lane. Kind is double-encoded — shape plus a short
 * mono label — never color alone: cmd = filled diamond, write = filled dot,
 * deny = warning triangle (--danger), workflow = square outline,
 * workflow_step = vertical tick. A failed (ok:false) non-deny event keeps its
 * shape but gets the --warn color and a " fail" label suffix. Labels alternate
 * between two rows and are skipped (never clipped mid-glyph) on collision;
 * the marker shapes themselves always draw.
 */
export function drawEventMarkers(
  ctx: CanvasRenderingContext2D,
  rect: OverlayRect,
  scales: TimeScales,
  events: readonly EventMarker[],
  cssVars: CssVarReader,
): void {
  const p = scales.pxRatio
  const { labelRowA, labelRowB, densityTop } = lanes(rect, p)
  clipped(ctx, rect, () => {
    ctx.font = monoFont(9 * p)
    ctx.textAlign = 'center'
    ctx.textBaseline = 'alphabetic'
    const rowEnds: Record<'a' | 'b', number> = { a: -Infinity, b: -Infinity }
    let drawn = 0
    for (const event of events) {
      // Shared geometry: the hover hit-test measures the same spot.
      const { x, y: markerY, r } = eventMarkerSpot(rect, scales, event.tsMs)
      if (x < rect.left - r || x > rect.left + rect.width + r) continue

      const failed = !event.ok && event.kind !== 'deny'
      // dashed leader from the marker lane down to the density band
      ctx.strokeStyle = cssVars('--scope-faint')
      ctx.lineWidth = 1 * p
      ctx.setLineDash([2 * p, 3 * p])
      ctx.beginPath()
      ctx.moveTo(x, markerY + 8 * p)
      ctx.lineTo(x, densityTop)
      ctx.stroke()
      ctx.setLineDash([])

      ctx.lineWidth = 1.5 * p
      switch (event.kind) {
        case 'cmd':
          ctx.fillStyle = cssVars(failed ? '--warn' : '--scope-ink')
          diamond(ctx, x, markerY, r)
          ctx.fill()
          break
        case 'write':
          ctx.fillStyle = cssVars(failed ? '--warn' : '--s2')
          ctx.beginPath()
          ctx.arc(x, markerY, 3.5 * p, 0, Math.PI * 2)
          ctx.fill()
          break
        case 'deny':
          ctx.strokeStyle = cssVars('--danger')
          warnTriangle(ctx, x, markerY, r)
          ctx.stroke()
          ctx.fillStyle = cssVars('--danger')
          ctx.fillRect(x - 0.75 * p, markerY - 2.5 * p, 1.5 * p, 4 * p)
          break
        case 'workflow':
          ctx.strokeStyle = cssVars(failed ? '--warn' : '--s4')
          ctx.strokeRect(x - 3.5 * p, markerY - 3.5 * p, 7 * p, 7 * p)
          break
        case 'workflow_step':
          ctx.fillStyle = cssVars(failed ? '--warn' : '--s4')
          ctx.fillRect(x - 0.75 * p, markerY - 4 * p, 1.5 * p, 8 * p)
          break
      }

      const row: 'a' | 'b' = drawn % 2 === 0 ? 'b' : 'a'
      drawn += 1
      const text = failed ? `${event.label} fail` : event.label
      const width = ctx.measureText(text).width
      if (x - width / 2 > rowEnds[row] + 6 * p) {
        ctx.fillStyle = cssVars(event.kind === 'deny' ? '--danger' : failed ? '--warn' : '--scope-muted')
        ctx.fillText(text, x, row === 'a' ? labelRowA : labelRowB)
        rowEnds[row] = x + width / 2
      }
    }
  })
}

/**
 * A/B measurement cursors: dashed --brand verticals with top caps; when both
 * are set, the window between them gets the neutral --scope-watermark shade
 * and the gap readout is drawn top-center. The caller must mirror the gap as
 * a persistent text KPI — the canvas readout is not the only copy.
 */
export function drawDualCursors(
  ctx: CanvasRenderingContext2D,
  rect: OverlayRect,
  scales: TimeScales,
  aMs: number | null,
  bMs: number | null,
  cssVars: CssVarReader,
): void {
  const p = scales.pxRatio
  const { gapTextY } = lanes(rect, p)
  const lineTop = rect.top + 18 * p
  const lineBottom = rect.top + rect.height
  clipped(ctx, rect, () => {
    if (aMs !== null && bMs !== null) {
      const x0 = timeToPx(rect, scales, Math.min(aMs, bMs))
      const x1 = timeToPx(rect, scales, Math.max(aMs, bMs))
      ctx.fillStyle = cssVars('--scope-watermark')
      ctx.fillRect(x0, lineTop, x1 - x0, lineBottom - lineTop)
    }
    const brand = cssVars('--brand')
    for (const [ms, name] of [
      [aMs, 'A'],
      [bMs, 'B'],
    ] as const) {
      if (ms === null) continue
      const x = timeToPx(rect, scales, ms)
      ctx.strokeStyle = brand
      ctx.lineWidth = 1 * p
      ctx.setLineDash([4 * p, 3 * p])
      ctx.beginPath()
      ctx.moveTo(x, lineTop)
      ctx.lineTo(x, lineBottom)
      ctx.stroke()
      ctx.setLineDash([])
      ctx.fillStyle = brand
      ctx.fillRect(x - 3 * p, lineTop, 6 * p, 2 * p)
      ctx.font = monoFont(9 * p, '700')
      ctx.textAlign = 'center'
      ctx.fillText(name, x, lineTop - 4 * p)
    }
    if (aMs !== null && bMs !== null) {
      const gap = Math.round(Math.abs(bMs - aMs))
      const mid = (timeToPx(rect, scales, aMs) + timeToPx(rect, scales, bMs)) / 2
      ctx.font = monoFont(10 * p, '700')
      ctx.textAlign = 'center'
      ctx.fillStyle = cssVars('--scope-ink')
      ctx.fillText(`gap ${gap} ms`, mid, gapTextY)
    }
  })
}
