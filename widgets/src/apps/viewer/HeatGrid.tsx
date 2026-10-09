// The heatmap surface shared by the live view (one grid per streamed frame)
// and the snapshot view (the one grid a saved result carries): a canvas
// painted by src/render/heat.ts, plus the measured min/max legend. The ramp
// tokens are read at paint time, so a host theme switch repaints through the
// same effect and the legend gradient (pure CSS, same tokens) always matches.
import { useEffect, useRef } from 'react'
import { cssVarReader } from '../../render/uplot-host'
import { drawHeatmapGrid, gridRange } from '../../render/heat'

const HEATMAP_HEIGHT = 280

export interface HeatCanvasProps {
  readonly rows: number
  readonly cols: number
  /** rows*cols values, row-major. */
  readonly values: readonly number[]
  /** The host theme — changing it repaints with the other theme's tokens. */
  readonly theme: unknown
}

export function HeatCanvas({ rows, cols, values, theme }: HeatCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const wrapRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    const wrap = wrapRef.current
    if (canvas === null || wrap === null) return
    const paint = (): void => {
      const dpr = window.devicePixelRatio || 1
      const width = Math.max(1, Math.floor(wrap.clientWidth))
      canvas.width = Math.floor(width * dpr)
      canvas.height = Math.floor(HEATMAP_HEIGHT * dpr)
      canvas.style.width = `${width}px`
      canvas.style.height = `${HEATMAP_HEIGHT}px`
      const ctx = canvas.getContext('2d')
      if (ctx === null) {
        throw new Error('heatmap: canvas 2d context unavailable')
      }
      drawHeatmapGrid(
        ctx,
        { rows, cols, width: canvas.width, height: canvas.height },
        values,
        cssVarReader(canvas),
      )
    }
    paint()
    const observer = new ResizeObserver(paint)
    observer.observe(wrap)
    return () => observer.disconnect()
  }, [rows, cols, values, theme])

  return (
    <div ref={wrapRef} className="heat-wrap">
      <canvas ref={canvasRef} className="heat-canvas" />
    </div>
  )
}

/** Placeholder in the canvas's place until a grid exists. */
export function HeatEmpty({ text }: { readonly text: string }) {
  return (
    <div className="heat-wrap">
      <div className="heat-empty">{text}</div>
    </div>
  )
}

export interface HeatLegendProps {
  /** The grid on screen, or undefined before the first one. */
  readonly values: readonly number[] | undefined
  readonly unit: string | undefined
}

/** Measured min/max of the grid on screen — the legend and cells agree. */
export function HeatLegend({ values, unit }: HeatLegendProps) {
  const range = values !== undefined ? gridRange(values) : undefined
  const withUnit = (value: number): string =>
    `${Number.isInteger(value) ? value : value.toFixed(2)}${unit !== undefined ? ` ${unit}` : ''}`
  return (
    <div className="viewer-kpi heat-legend">
      <span>
        <i className="heat-ramp" />
        {range !== undefined ? (
          <>
            min <b>{withUnit(range.min)}</b> · max <b>{withUnit(range.max)}</b>
          </>
        ) : (
          <>no grid yet</>
        )}
      </span>
      {unit !== undefined && (
        <span>
          unit <b>{unit}</b>
        </span>
      )}
    </div>
  )
}
