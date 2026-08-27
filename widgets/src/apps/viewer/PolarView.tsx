import { useCallback, useEffect, useMemo, useRef } from 'react'
import { BadgeSim } from '../../components/Badges'
import { Btn } from '../../components/Btn'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { Icon } from '../../components/Icon'
import { Led } from '../../components/Led'
import { ObError } from '../../components/ObError'
import { ObEmpty } from '../../components/ObEmpty'
import type { ToolArgs, ToolResult, WidgetHandle } from '../../mcp/useWidget'
import { drawPolar, type PolarFrame, type PolarPoint } from '../../render/polar'
import { radarScale } from './dispatch'
import { intensityRange, type RadarState } from './state'
import { RATE_OPTIONS, useRadarLoop } from './useRadarLoop'
import { useReducedMotion } from './useReducedMotion'

const RAMP_TOKENS = ['--ramp-g1', '--ramp-g2', '--ramp-g3', '--ramp-g4', '--ramp-g5'] as const
const POLL_DISABLED_REASON =
  'the host did not provide the originating tool call — continuous scan unavailable'

export interface PolarViewProps {
  readonly widget: WidgetHandle
  readonly radar: RadarState
  readonly toolName: string | undefined
  readonly toolArgs: ToolArgs
  readonly onResult: (result: ToolResult) => void
  readonly onFailure: (message: string) => void
}

function formatAngle(angleDeg: number): string {
  return `${Number.isInteger(angleDeg) ? angleDeg : angleDeg.toFixed(1)}°`
}

/** Radar panel per the w02 design card: scope, corner meta, ramp, controls. */
export function PolarView({ widget, radar, toolName, toolArgs, onResult, onFailure }: PolarViewProps) {
  const reducedMotion = useReducedMotion()
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const scan = radar.current

  const canPoll = toolName !== undefined && toolArgs !== undefined
  const poll = useCallback(async (): Promise<boolean> => {
    if (toolName === undefined) {
      onFailure(`cannot poll — ${POLL_DISABLED_REASON}`)
      return false
    }
    try {
      onResult(await widget.callTool(toolName, toolArgs))
      return true
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      onFailure(`tool call failed: ${message} — auto-scan paused`)
      return false
    }
  }, [widget, toolName, toolArgs, onResult, onFailure])
  const loop = useRadarLoop({ canPoll, reducedMotion, poll })

  const range = useMemo(() => intensityRange(radar), [radar])
  const scale = useMemo(() => {
    const points: PolarPoint[] = [...(scan?.points ?? []), ...(radar.ghost?.points ?? [])]
    return radarScale(points)
  }, [scan, radar.ghost])
  const frame: PolarFrame | undefined = useMemo(
    () =>
      scan === undefined
        ? undefined
        : {
            points: scan.points,
            ghost: radar.ghost?.points,
            intensityMin: range.min,
            intensityMax: range.max,
          },
    [scan, radar.ghost, range],
  )

  const draw = useCallback(
    (sweepDeg: number | null): void => {
      const canvas = canvasRef.current
      if (!canvas || frame === undefined) return
      drawPolar(canvas, frame, { ...scale, sweepDeg, annotateNearest: true })
    },
    [frame, scale],
  )

  // Frozen-frame sweep angle: where the newest data ends (design default).
  const staticSweepDeg = scan?.points.at(-1)?.angleDeg ?? null

  const animating = loop.playing && !reducedMotion && frame !== undefined
  useEffect(() => {
    if (!animating) {
      draw(staticSweepDeg)
      return
    }
    let raf = 0
    const start = performance.now()
    const base = staticSweepDeg ?? 0
    const tick = (now: number): void => {
      // One revolution per poll period, so the sweep tracks the chosen rate.
      draw((base + ((now - start) / 1000) * loop.rateHz * 360) % 360)
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [animating, draw, staticSweepDeg, loop.rateHz])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const observer = new ResizeObserver(() => draw(staticSweepDeg))
    observer.observe(canvas)
    return () => observer.disconnect()
  }, [draw, staticSweepDeg])

  const modes = widget.hostContext?.availableDisplayModes
  const fullscreen = widget.displayMode === 'fullscreen'
  const canFullscreen = fullscreen || modes === undefined || modes.includes('fullscreen')
  const onFullscreen = useCallback((): void => {
    widget.requestDisplayMode(fullscreen ? 'inline' : 'fullscreen').catch((error: unknown) => {
      const message = error instanceof Error ? error.message : String(error)
      onFailure(`display mode request failed: ${message}`)
    })
  }, [widget, fullscreen, onFailure])

  if (scan === undefined) {
    // ingestResult only enters polar state with a good frame; keep the guard loud.
    return (
      <div className="viewer-root">
        <Card title="Radar">
          <ObEmpty>No radar frame received yet.</ObEmpty>
        </Card>
      </div>
    )
  }

  const nearest = scan.points.reduce((a, b) => (b.distanceMm < a.distanceMm ? b : a))
  const blParts: string[] = []
  if (scan.uptimeMs !== undefined) blParts.push(`uptime ${(scan.uptimeMs / 1000).toFixed(1)}s`)
  blParts.push(
    `${scan.points.length} pts${scan.truncatedPoints > 0 ? ` (+${scan.truncatedPoints} truncated)` : ''}`,
  )
  const fullscreenTitle = fullscreen ? 'Exit fullscreen' : 'Fullscreen'
  const port = typeof toolArgs?.port === 'string' ? toolArgs.port : undefined
  const sessionId = typeof toolArgs?.session_id === 'string' ? toolArgs.session_id : undefined

  return (
    <div className="viewer-root">
      <Card
        title={`Radar — ${scan.command ?? 'scan'}`}
        head={
          <>
            {scan.simulatedScene && <BadgeSim />}
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
            <Led
              tone={radar.error !== undefined ? 'warn' : loop.playing ? 'ok' : 'off'}
              pulse={loop.playing}
            />
            <span className="live-tag">{loop.playing ? 'LIVE' : 'PAUSED'}</span>
          </>
        }
        foot={
          <>
            <Btn
              onClick={loop.toggle}
              disabled={!canPoll}
              title={canPoll ? undefined : POLL_DISABLED_REASON}
            >
              <Icon name={loop.playing ? 'pause' : 'play'} />
              {loop.playing ? 'Pause' : 'Resume'}
            </Btn>
            <div className="ob-seg" role="group" aria-label="Poll rate">
              {RATE_OPTIONS.map((rate) => (
                <button
                  key={rate}
                  type="button"
                  className={rate === loop.rateHz ? 'is-on' : undefined}
                  aria-pressed={rate === loop.rateHz}
                  disabled={!canPoll}
                  title={canPoll ? undefined : POLL_DISABLED_REASON}
                  onClick={() => loop.setRate(rate)}
                >
                  {rate} Hz
                </button>
              ))}
            </div>
            <CardSpacer />
            {port !== undefined && (
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>{port}</span>
            )}
            {sessionId !== undefined && <Chip>session: {sessionId}</Chip>}
          </>
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
            <span>{loop.playing ? `poll ${loop.rateHz.toFixed(1)} Hz` : 'paused'}</span>
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
        {radar.error !== undefined && (
          <div style={{ marginTop: 10 }}>
            <ObError
              title="Scan failed"
              detail={radar.error}
              onRetry={!loop.playing && canPoll ? loop.resume : undefined}
              retryLabel="Resume"
            />
          </div>
        )}
      </Card>
    </div>
  )
}
