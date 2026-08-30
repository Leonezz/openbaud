import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { BadgeReplay, BadgeSim } from '../../components/Badges'
import { Btn } from '../../components/Btn'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { Icon } from '../../components/Icon'
import { Led } from '../../components/Led'
import { ObError } from '../../components/ObError'
import type { WidgetHandle } from '../../mcp/useWidget'
import {
  drawPolar,
  nearestPolarPoint,
  polarLayout,
  polarPointXy,
  type PolarFrame,
  type PolarPoint,
} from '../../render/polar'
import { createTooltipHost, type TooltipHost, type TooltipLine } from '../../render/tooltip'
import { radarScale, type PolarScan } from './dispatch'
import { intensityRange, type ResultRef } from './state'

const RAMP_TOKENS = ['--ramp-g1', '--ramp-g2', '--ramp-g3', '--ramp-g4', '--ramp-g5'] as const
/** Mouse-to-point hit radius in CSS pixels. */
const HIT_RADIUS_PX = 12

/**
 * Tooltip rows under the device's own field names — exactly the names the
 * declared encoding mapped onto each channel, the same route the dots were
 * drawn from. A unit is appended only when the result's `units` map declares
 * one for that field; an undeclared intensity channel contributes no row at
 * all (its defaulted 0 is not device data).
 */
function pointTooltip(scan: PolarScan, point: PolarPoint): readonly TooltipLine[] {
  const valueOf = (field: string, value: number): string => {
    const unit = scan.units[field]
    return unit === undefined ? String(value) : `${value} ${unit}`
  }
  return [
    { label: scan.channels.angle, value: valueOf(scan.channels.angle, point.angleDeg) },
    { label: scan.channels.radius, value: valueOf(scan.channels.radius, point.distanceMm) },
    ...(scan.channels.intensity !== undefined
      ? [{ label: scan.channels.intensity, value: valueOf(scan.channels.intensity, point.intensity) }]
      : []),
  ]
}

export interface PolarViewProps {
  readonly widget: WidgetHandle
  readonly scan: PolarScan
  readonly resultRef: ResultRef
}

function formatAngle(angleDeg: number): string {
  return `${Number.isInteger(angleDeg) ? angleDeg : angleDeg.toFixed(1)}°`
}

/**
 * Radar panel per the w02 design card: scope, corner meta, ramp.
 *
 * One saved scan, drawn once. There is no animation loop here — the sweep line
 * is parked at the newest bearing and the frame carries no afterglow, because a
 * stored result has no predecessor to fade. That leaves the prefers-reduced-
 * motion policy entirely to the CSS block in theme/widget.css: no JS-driven
 * motion exists to freeze.
 */
export function PolarView({ widget, scan, resultRef }: PolarViewProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const scopeRef = useRef<HTMLDivElement | null>(null)
  const tooltipRef = useRef<TooltipHost | null>(null)
  const [modeError, setModeError] = useState<string | undefined>(undefined)
  const [hover, setHover] = useState<number | null>(null)

  const range = useMemo(() => intensityRange(scan), [scan])
  const scale = useMemo(() => radarScale(scan.points), [scan])
  const frame: PolarFrame = useMemo(
    () => ({ points: scan.points, intensityMin: range.min, intensityMax: range.max }),
    [scan, range],
  )

  // Frozen-frame sweep angle: where the newest data ends (design default).
  const sweepDeg = scan.points.at(-1)?.angleDeg ?? null

  // The render core reads every colour from theme tokens at draw time, so a
  // host theme switch must redraw — the canvas would otherwise keep the
  // previous theme's palette.
  const theme = widget.theme
  const draw = useCallback((): void => {
    const canvas = canvasRef.current
    if (!canvas) return
    void theme
    drawPolar(canvas, frame, { ...scale, sweepDeg, annotateNearest: true, highlightIndex: hover })
  }, [frame, scale, sweepDeg, theme, hover])

  useEffect(() => {
    draw()
  }, [draw])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const observer = new ResizeObserver(() => draw())
    observer.observe(canvas)
    return () => observer.disconnect()
  }, [draw])

  // One tooltip host per scope surface; content is set per hover below.
  useEffect(() => {
    const scope = scopeRef.current
    if (!scope) return
    const tooltip = createTooltipHost(scope)
    tooltipRef.current = tooltip
    return () => {
      tooltipRef.current = null
      tooltip.destroy()
    }
  }, [])

  // Per-point hover: the hit-test runs through the same polarLayout /
  // polarPointXy math that painted the dots (render/polar.ts), so the point
  // that looks nearest is the one that hits. Beyond HIT_RADIUS_PX: no point,
  // no ring, no tooltip.
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const onMove = (event: MouseEvent): void => {
      const box = canvas.getBoundingClientRect()
      const layout = polarLayout(box.width, box.height, scale.maxDistanceMm)
      const index = nearestPolarPoint(
        layout,
        scan.points,
        event.clientX - box.left,
        event.clientY - box.top,
        HIT_RADIUS_PX,
      )
      setHover(index)
      const tooltip = tooltipRef.current
      if (tooltip === null) return
      const point = index === null ? undefined : scan.points[index]
      if (point === undefined) {
        tooltip.hide()
        return
      }
      // The canvas fills the scope (inset 0), so canvas and tooltip-host
      // coordinates are the same space.
      const at = polarPointXy(layout, point)
      tooltip.show(at.x, at.y, pointTooltip(scan, point))
    }
    const onLeave = (): void => {
      setHover(null)
      tooltipRef.current?.hide()
    }
    canvas.addEventListener('mousemove', onMove)
    canvas.addEventListener('mouseleave', onLeave)
    return () => {
      canvas.removeEventListener('mousemove', onMove)
      canvas.removeEventListener('mouseleave', onLeave)
    }
  }, [scan, scale])

  const modes = widget.hostContext?.availableDisplayModes
  const fullscreen = widget.displayMode === 'fullscreen'
  const canFullscreen = fullscreen || modes === undefined || modes.includes('fullscreen')
  const onFullscreen = useCallback((): void => {
    setModeError(undefined)
    widget.requestDisplayMode(fullscreen ? 'inline' : 'fullscreen').catch((error: unknown) => {
      const detail = error instanceof Error ? error.message : String(error)
      setModeError(`display mode request failed: ${detail}`)
    })
  }, [widget, fullscreen])

  const nearest = scan.points.reduce((a, b) => (b.distanceMm < a.distanceMm ? b : a))
  const outcomeOk = scan.outcome === undefined || scan.outcome === 'normal'
  const blParts: string[] = []
  if (scan.uptimeMs !== undefined) blParts.push(`uptime ${(scan.uptimeMs / 1000).toFixed(1)}s`)
  blParts.push(
    `${scan.points.length} pts${scan.truncatedPoints > 0 ? ` (+${scan.truncatedPoints} truncated)` : ''}`,
  )
  const fullscreenTitle = fullscreen ? 'Exit fullscreen' : 'Fullscreen'
  const sourceLabel = resultRef.source === 'file' ? 'saved result' : 'inline result'
  const hasProvenance = resultRef.path !== undefined || resultRef.bytes !== undefined

  return (
    <div className="viewer-root">
      <Card
        title={`Radar — ${scan.command ?? 'scan'}`}
        head={
          <>
            {scan.replay && <BadgeReplay />}
            {scan.simulatedScene && <BadgeSim />}
            {!outcomeOk && <Chip variant="warn">outcome: {scan.outcome}</Chip>}
            <CardSpacer />
            {canFullscreen && (
              <Btn
                variant="ghost"
                className="ob-btn--icon"
                title={fullscreenTitle}
                aria-label={fullscreenTitle}
                onClick={onFullscreen}
              >
                <Icon name="expand" />
              </Btn>
            )}
            <Led tone={outcomeOk ? 'ok' : 'warn'} />
            <span className="live-tag">SNAPSHOT</span>
          </>
        }
        foot={
          // An inline result has neither path nor size; skip the bar entirely
          // rather than leave an empty strip under the scope.
          hasProvenance ? (
            <>
              {resultRef.path !== undefined && (
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
                  {resultRef.path}
                </span>
              )}
              <CardSpacer />
              {resultRef.bytes !== undefined && <Chip>{resultRef.bytes} B</Chip>}
            </>
          ) : undefined
        }
      >
        <div ref={scopeRef} className="ob-scope ob-scope--radar">
          <canvas ref={canvasRef} className="radar" />
          {/* Replayed captures are disclosed on the surface itself, not only
              in the header — the scope is what screenshots travel as. */}
          {scan.replay && (
            <div className="ob-watermark">
              <span>REPLAY</span>
            </div>
          )}
          <div className="ob-scope__meta ob-scope__meta--tl">
            <span>
              {[scan.protocol, scan.device ?? 'unknown device']
                .filter((part) => part !== undefined)
                .join(' · ')}
            </span>
          </div>
          <div className="ob-scope__meta ob-scope__meta--tr">
            {/* Shown only when the server reports a checksum it actually ran:
                a `normal` outcome on a command with no `validate` verifies
                nothing, and claiming otherwise on screen would be a lie. */}
            {scan.checksum !== undefined && (
              <span className="is-pass">{scan.checksum} ✓</span>
            )}
            {scan.seq !== undefined && <span>seq {scan.seq}</span>}
          </div>
          <div className="ob-scope__meta ob-scope__meta--bl">
            <span>{blParts.join(' · ')}</span>
          </div>
          <div className="ob-scope__meta ob-scope__meta--br">
            <span>{sourceLabel}</span>
          </div>
          {scan.hasIntensity && (
            <div className="ob-ramp">
              <span className="t">INTENSITY</span>
              <span className="v">{range.min}</span>
              {RAMP_TOKENS.map((token) => (
                <i key={token} style={{ background: `var(${token})` }} />
              ))}
              <span className="v">{range.max}</span>
            </div>
          )}
        </div>
        <div className="viewer-kpi">
          <span>
            nearest <b>{Math.round(nearest.distanceMm)} mm</b> @ {formatAngle(nearest.angleDeg)}
          </span>
        </div>
        {modeError !== undefined && (
          <div style={{ marginTop: 10 }}>
            <ObError
              title="Display mode unchanged"
              detail={modeError}
              onRetry={onFullscreen}
            />
          </div>
        )}
      </Card>
    </div>
  )
}
