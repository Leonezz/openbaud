import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import type { OpenbaudSummary } from '../../mcp/useWidget'
import { JsonKv } from './JsonKv'
import type { ResultRef } from './state'

export interface GenericViewProps {
  readonly structured: OpenbaudSummary
  readonly resultRef: ResultRef
}

/** Generic fallback: any non-polar result as a recursive key/value card. */
export function GenericView({ structured, resultRef }: GenericViewProps) {
  const command = typeof structured.command === 'string' ? structured.command : undefined
  const outcome = typeof structured.outcome === 'string' ? structured.outcome : undefined
  const title = command !== undefined ? `Result — ${command}` : 'Result'
  const ok = outcome === 'normal' || outcome === 'sent'
  const provenance =
    resultRef.path ?? (resultRef.source === 'file' ? resultRef.uri : 'inline result')
  return (
    <div className="viewer-root">
      <Card
        title={title}
        head={
          outcome !== undefined ? (
            <>
              <CardSpacer />
              <Chip variant={ok ? 'accent' : 'warn'}>outcome: {outcome}</Chip>
            </>
          ) : undefined
        }
        foot={
          <>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>{provenance}</span>
            <CardSpacer />
            {resultRef.bytes !== undefined && <Chip>{resultRef.bytes} B</Chip>}
          </>
        }
      >
        <JsonKv value={structured} />
      </Card>
    </div>
  )
}
