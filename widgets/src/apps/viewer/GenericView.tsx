import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import type { OpenbaudSummary } from '../../mcp/useWidget'
import { JsonKv } from './JsonKv'

export interface GenericViewProps {
  readonly structured: OpenbaudSummary
  readonly toolName: string | undefined
}

/** Generic fallback: any non-polar tool result as a recursive key/value card. */
export function GenericView({ structured, toolName }: GenericViewProps) {
  const command = typeof structured.command === 'string' ? structured.command : undefined
  const outcome = typeof structured.outcome === 'string' ? structured.outcome : undefined
  const fullResult = typeof structured.full_result === 'string' ? structured.full_result : undefined
  const title =
    command !== undefined
      ? `Result — ${command}`
      : toolName !== undefined
        ? `Result — ${toolName}`
        : 'Tool result'
  const ok = outcome === 'normal' || outcome === 'sent'
  const body = Object.fromEntries(Object.entries(structured).filter(([key]) => key !== 'full_result'))
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
          fullResult !== undefined ? (
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
              full result: {fullResult}
            </span>
          ) : undefined
        }
      >
        <JsonKv value={body} />
      </Card>
    </div>
  )
}
