import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { BadgeSim } from '../../components/Badges'
import { Btn } from '../../components/Btn'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { Icon } from '../../components/Icon'
import { Led } from '../../components/Led'
import { ObError } from '../../components/ObError'
import type { WidgetHandle } from '../../mcp/useWidget'
import { drawPolar, type PolarFrame } from '../../render/polar'
import { radarScale, type PolarScan } from './dispatch'
import { intensityRange, type ResultRef } from './state'

const RAMP_TOKENS = ['--ramp-g1', '--ramp-g2', '--ramp-g3', '--ramp-g4', '--ramp-g5'] as const

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
  const [modeError, setModeError] = useState<string | undefined>(undefined)

  const range = useMemo(() => intensityRange(scan), [scan])
  const scale = useMemo(() => radarScale(scan.points), [scan])
  const frame: PolarFrame = useMemo(
    () => ({ points: scan.points, intensityMin: range.min, intensityMax: range.max }),
    [scan, range],
  )

  // Frozen-frame sweep angle: where the newest data ends (design default).
  const sweepDeg = scan.points.at(-1)?.angleDeg ?? null

  const draw = useCallback((): void => {
    const canvas = canvasRef.current
    if (!canvas) return
    drawPolar(canvas, frame, { ...scale, sweepDeg, annotateNearest: true })
  }, [frame, scale, sweepDeg])

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
        <div className="ob-scope ob-scope--radar">
          <canvas ref={canvasRef} className="radar" />
          <div className="ob-scope__meta ob-scope__meta--tl">
            <span>
              {[scan.protocol, scan.device ?? 'unknown device']
                .filter((part) => part !== undefined)
                .join(' · ')}
            </span>
          </div>
          <div className="ob-scope__meta ob-scope__meta--tr">
            {scan.outcome === 'normal' && <span className="is-pass">CRC PASS ✓</span>}
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
