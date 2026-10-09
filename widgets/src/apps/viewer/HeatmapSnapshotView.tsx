// A saved result that declares view: { kind: "heatmap" } — the one grid the
// result carries, drawn through the same heat grid as the live view. A
// replayed result is disclosed on the surface itself, as on the radar.
import { BadgeReplay } from '../../components/Badges'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { Led } from '../../components/Led'
import type { WidgetHandle } from '../../mcp/useWidget'
import { HeatCanvas, HeatLegend } from './HeatGrid'
import type { HeatmapSnapshot } from './heatmapSnapshot'
import type { ResultRef } from './state'

export interface HeatmapSnapshotViewProps {
  readonly widget: WidgetHandle
  readonly heatmap: HeatmapSnapshot
  readonly replay: boolean
  readonly resultRef: ResultRef
}

export function HeatmapSnapshotView({ widget, heatmap, replay, resultRef }: HeatmapSnapshotViewProps) {
  const hasProvenance = resultRef.path !== undefined || resultRef.bytes !== undefined
  return (
    <div className="viewer-root">
      <Card
        title="Heatmap"
        head={
          <>
            {heatmap.source !== undefined && <Chip>{heatmap.source}</Chip>}
            {replay && <BadgeReplay />}
            <CardSpacer />
            <Led tone="ok" />
            <span className="live-tag">SNAPSHOT</span>
          </>
        }
        foot={
          hasProvenance ? (
            <>
              {resultRef.path !== undefined && (
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>{resultRef.path}</span>
              )}
              <CardSpacer />
              {resultRef.bytes !== undefined && <Chip>{resultRef.bytes} B</Chip>}
            </>
          ) : undefined
        }
      >
        <div className="ob-scope ob-scope--heatmap">
          <div className="ob-scope__meta ob-scope__meta--tl">
            <span>
              {heatmap.data} <b>{heatmap.rows}×{heatmap.cols}</b>
            </span>
          </div>
          <div className="ob-scope__meta ob-scope__meta--br">
            <span>{resultRef.source === 'file' ? 'saved result' : 'inline result'}</span>
          </div>
          <HeatCanvas
            rows={heatmap.rows}
            cols={heatmap.cols}
            values={heatmap.values}
            theme={widget.theme}
          />
          {replay && (
            <div className="ob-watermark">
              <span>REPLAY</span>
            </div>
          )}
        </div>
        <HeatLegend values={heatmap.values} unit={heatmap.unit} />
      </Card>
    </div>
  )
}
