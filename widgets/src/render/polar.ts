// Pure polar/radar canvas renderer, extracted from the design SoT
// (docs/design/mcp-apps-ui/library/widgets/w02-radar-panel.html).
// Colors are read from the theme tokens at draw time via getComputedStyle, so
// the only DOM requirement is that the canvas sits under a token-bearing root.
// Animation policy lives with the caller: under prefers-reduced-motion the
// caller draws a single frozen frame (sweepDeg: null) and keeps live values
// as text KPIs.

export interface PolarPoint {
  readonly angleDeg: number
  readonly distanceMm: number
  readonly intensity: number
}

export interface PolarFrame {
  /** Current scan, full opacity, intensity-ramp colored. */
  readonly points: readonly PolarPoint[]
  /** Previous scan, drawn as a 30% afterglow. */
  readonly ghost?: readonly PolarPoint[]
  /** Intensity range mapped onto the 5-step ramp (mirror it in a text KPI). */
  readonly intensityMin: number
  readonly intensityMax: number
}

export interface PolarSector {
  readonly fromDeg: number
  readonly toDeg: number
  readonly fromMm: number
  readonly toMm: number
}

export interface PolarOptions {
  /** Drawing boundary; must exceed the farthest expected point. */
  readonly maxDistanceMm?: number
  /** Outermost labelled range ring. */
  readonly ringMaxMm?: number
  readonly ringStepMm?: number
  /** Sweep line angle in degrees; null/undefined draws no sweep. */
  readonly sweepDeg?: number | null
  /** Thin out ring labels and shrink dots for small canvases. */
  readonly compact?: boolean
  /** Direct-label the nearest target (scope ink, never a series color). */
  readonly annotateNearest?: boolean
  /** Dashed selection-sector overlay (selection-layer entry point). */
  readonly sector?: PolarSector | null
}

const RAMP_TOKENS = ['--ramp-g1', '--ramp-g2', '--ramp-g3', '--ramp-g4', '--ramp-g5'] as const
const MONO_FONT = '9px ui-monospace, Menlo, monospace'

function hexAlpha(hex: string, alpha: number): string {
  const n = parseInt(hex.replace('#', ''), 16)
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${alpha})`
}

/** 0° points up, angles grow clockwise (full circle). */
function toRad(deg: number): number {
  return ((deg - 90) * Math.PI) / 180
}

export function drawPolar(canvas: HTMLCanvasElement, frame: PolarFrame, opts: PolarOptions = {}): void {
  const {
    maxDistanceMm = 2800,
    ringMaxMm = 2500,
    ringStepMm = 500,
    sweepDeg = null,
    compact = false,
    annotateNearest = false,
    sector = null,
  } = opts

  const rect = canvas.getBoundingClientRect()
  if (rect.width < 10 || rect.height < 10) {
    // Not laid out yet; the caller redraws on resize.
    return
  }
  const ctx2d = canvas.getContext('2d')
  if (!ctx2d) {
    throw new Error('drawPolar: canvas 2d context unavailable')
  }
  const style = getComputedStyle(canvas)
  const token = (name: string): string => {
    const value = style.getPropertyValue(name).trim()
    if (!value) {
      throw new Error(`drawPolar: token ${name} is not defined — import src/theme/index.css`)
    }
    return value
  }
  const ramp = RAMP_TOKENS.map(token)

  const dpr = window.devicePixelRatio || 1
  canvas.width = Math.round(rect.width * dpr)
  canvas.height = Math.round(rect.height * dpr)
  ctx2d.scale(dpr, dpr)

  const w = rect.width
  const h = rect.height
  const cx = w / 2
  const cy = h / 2
  const radius = Math.min(w, h) / 2 - (compact ? 18 : 26)
  const px = (mm: number): number => (radius * mm) / maxDistanceMm

  const { intensityMin, intensityMax } = frame
  const binColor = (intensity: number): string => {
    const span = intensityMax - intensityMin + 1
    const bin = Math.max(0, Math.min(4, Math.floor(((intensity - intensityMin) / span) * 5)))
    const color = ramp[bin]
    if (color === undefined) {
      throw new Error(`drawPolar: intensity ${intensity} mapped outside the 5-step ramp`)
    }
    return color
  }

  // 30° spokes
  ctx2d.strokeStyle = token('--scope-grid')
  ctx2d.lineWidth = 1
  for (let a = 0; a < 360; a += 30) {
    ctx2d.beginPath()
    ctx2d.moveTo(cx, cy)
    ctx2d.lineTo(cx + px(ringMaxMm) * Math.cos(toRad(a)), cy + px(ringMaxMm) * Math.sin(toRad(a)))
    ctx2d.stroke()
  }

  // range rings
  ctx2d.strokeStyle = token('--scope-line')
  for (let mm = ringStepMm; mm <= ringMaxMm; mm += ringStepMm) {
    ctx2d.beginPath()
    ctx2d.arc(cx, cy, px(mm), 0, Math.PI * 2)
    ctx2d.stroke()
  }

  // ring labels (mono, --scope-faint; thinned when compact)
  ctx2d.font = MONO_FONT
  ctx2d.fillStyle = token('--scope-faint')
  ctx2d.textAlign = 'left'
  for (let mm = ringStepMm; mm <= ringMaxMm; mm += ringStepMm) {
    if (compact && (mm === ringStepMm || mm === ringStepMm * 3)) continue
    ctx2d.fillText(mm === ringMaxMm ? `${mm} mm` : String(mm), cx + 4, cy - px(mm) + 10)
  }

  // bearing labels
  ctx2d.textAlign = 'center'
  for (const a of [0, 90, 180, 270]) {
    const r = px(ringMaxMm) + 11
    ctx2d.fillText(`${a}°`, cx + r * Math.cos(toRad(a)), cy + r * Math.sin(toRad(a)) + 3)
  }

  // sweep line: brand green with a 70° conic trail
  if (sweepDeg !== null && sweepDeg !== undefined) {
    const angle = toRad(sweepDeg)
    const trail = (70 * Math.PI) / 180
    const brand = token('--brand')
    if (ctx2d.createConicGradient) {
      const gradient = ctx2d.createConicGradient(angle - trail, cx, cy)
      const f = trail / (2 * Math.PI)
      gradient.addColorStop(0, hexAlpha(brand, 0))
      gradient.addColorStop(f, hexAlpha(brand, 0.2))
      gradient.addColorStop(Math.min(1, f + 0.001), hexAlpha(brand, 0))
      gradient.addColorStop(1, hexAlpha(brand, 0))
      ctx2d.fillStyle = gradient
      ctx2d.beginPath()
      ctx2d.moveTo(cx, cy)
      ctx2d.arc(cx, cy, px(ringMaxMm), angle - trail, angle)
      ctx2d.closePath()
      ctx2d.fill()
    }
    ctx2d.strokeStyle = brand
    ctx2d.lineWidth = 1.5
    ctx2d.shadowColor = token('--brand-glow')
    ctx2d.shadowBlur = 6
    ctx2d.beginPath()
    ctx2d.moveTo(cx, cy)
    ctx2d.lineTo(cx + px(ringMaxMm) * Math.cos(angle), cy + px(ringMaxMm) * Math.sin(angle))
    ctx2d.stroke()
    ctx2d.shadowBlur = 0
  }

  const dot = (p: PolarPoint, r: number): void => {
    const rr = px(Math.min(p.distanceMm, maxDistanceMm - 10))
    const x = cx + rr * Math.cos(toRad(p.angleDeg))
    const y = cy + rr * Math.sin(toRad(p.angleDeg))
    ctx2d.fillStyle = binColor(p.intensity)
    ctx2d.beginPath()
    ctx2d.arc(x, y, r, 0, Math.PI * 2)
    ctx2d.fill()
  }

  // previous-frame afterglow at 30%
  if (frame.ghost) {
    ctx2d.globalAlpha = 0.3
    for (const p of frame.ghost) dot(p, compact ? 2 : 2.4)
    ctx2d.globalAlpha = 1
  }

  // main frame; top intensity bin gets a soft glow
  for (const p of frame.points) {
    if (p.intensity >= intensityMin + (intensityMax - intensityMin) * 0.8) {
      ctx2d.shadowColor = token('--brand-glow')
      ctx2d.shadowBlur = 7
    }
    dot(p, compact ? 2.4 : 3)
    ctx2d.shadowBlur = 0
  }

  // direct label on the nearest target (ink token, not a series color)
  if (annotateNearest && frame.points.length > 0) {
    const nearest = frame.points.reduce((a, b) => (b.distanceMm < a.distanceMm ? b : a))
    const rr = px(nearest.distanceMm)
    const x = cx + rr * Math.cos(toRad(nearest.angleDeg))
    const y = cy + rr * Math.sin(toRad(nearest.angleDeg))
    ctx2d.strokeStyle = token('--scope-faint')
    ctx2d.lineWidth = 1
    ctx2d.beginPath()
    ctx2d.moveTo(x + 4, y - 4)
    ctx2d.lineTo(x + 10, y - 10)
    ctx2d.stroke()
    ctx2d.font = MONO_FONT
    ctx2d.fillStyle = token('--scope-ink')
    ctx2d.textAlign = 'left'
    ctx2d.fillText(`${nearest.distanceMm} mm @ ${nearest.angleDeg}°`, x + 12, y - 12)
  }

  // dashed selection sector (selection-layer entry point)
  if (sector) {
    const a1 = toRad(sector.fromDeg)
    const a2 = toRad(sector.toDeg)
    const r1 = px(sector.fromMm)
    const r2 = px(sector.toMm)
    ctx2d.setLineDash([4, 3])
    ctx2d.strokeStyle = hexAlpha(token('--brand'), 0.55)
    ctx2d.lineWidth = 1
    ctx2d.beginPath()
    ctx2d.arc(cx, cy, r2, a1, a2)
    ctx2d.lineTo(cx + r1 * Math.cos(a2), cy + r1 * Math.sin(a2))
    ctx2d.arc(cx, cy, r1, a2, a1, true)
    ctx2d.closePath()
    ctx2d.stroke()
    ctx2d.setLineDash([])
  }
}
