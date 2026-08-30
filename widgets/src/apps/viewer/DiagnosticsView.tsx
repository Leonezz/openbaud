import { Fragment, useCallback, useMemo, useState } from 'react'
import { BadgeReplay } from '../../components/Badges'
import { Btn } from '../../components/Btn'
import { Card, CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { ObError } from '../../components/ObError'
import { ObTable, ObTableRow } from '../../components/ObTable'
import { Receipt } from '../../components/Receipt'
import type { WidgetHandle } from '../../mcp/useWidget'
import { checksumSpan, type DiagnosticsData, type ParseAttempt } from './diagnostics'
import type { ResultRef } from './state'

type SendState =
  | { readonly kind: 'idle' }
  | { readonly kind: 'sending' }
  | { readonly kind: 'sent'; readonly summary: string }
  | { readonly kind: 'failed'; readonly message: string }

function hex2(byte: number): string {
  return byte.toString(16).toUpperCase().padStart(2, '0')
}

/** Compact `k=v` rendering of a parsed attempt's decoded fields. */
function fieldsText(fields: Record<string, unknown>): string {
  return Object.entries(fields)
    .map(([key, value]) => {
      const text =
        typeof value === 'number' || typeof value === 'string' || typeof value === 'boolean'
          ? String(value)
          : JSON.stringify(value)
      return `${key}=${text}`
    })
    .join(' · ')
}

export interface DiagnosticsViewProps {
  readonly widget: WidgetHandle
  readonly diagnostics: DiagnosticsData
  readonly resultRef: ResultRef
}

/**
 * Frame-diagnostics card per the W12 design card: the frame's bytes in the
 * .ob-hex surface, the checksum hypothesis matrix, and (when the tool ran an
 * offset scan) the parse attempts. Row and offset clicks only move a local
 * highlight over the hex — the full payload is already here, so no tool is
 * ever called. The one outbound action is Adopt, which hands the winning
 * {checksum, offset} pair to the agent via updateModelContext.
 */
export function DiagnosticsView({ widget, diagnostics, resultRef }: DiagnosticsViewProps) {
  const { bytes, frameLen, matrix, attempts } = diagnostics

  // The winners are data, not selection: the first checksum the tool verified,
  // and the first offset whose bytes decode structurally (parsed says only
  // that — the probed offsets are mutually exclusive hypotheses). Selection
  // below is only a viewing aid for the hex.
  const winnerRow = useMemo(() => matrix.find((row) => row.ok), [matrix])
  const winnerAttempt = useMemo(() => attempts?.find((attempt) => attempt.parsed), [attempts])

  const firstOkRow = useMemo(() => matrix.findIndex((row) => row.ok), [matrix])
  const firstParsedAttempt = useMemo(
    () => (attempts === undefined ? -1 : attempts.findIndex((attempt) => attempt.parsed)),
    [attempts],
  )
  const [selRow, setSelRow] = useState<number | null>(firstOkRow >= 0 ? firstOkRow : null)
  const [selAttempt, setSelAttempt] = useState<number | null>(
    firstParsedAttempt >= 0 ? firstParsedAttempt : null,
  )
  const [send, setSend] = useState<SendState>({ kind: 'idle' })

  const row = selRow === null ? undefined : matrix[selRow]
  const attempt = selAttempt === null ? undefined : attempts?.[selAttempt]

  const byteClass = (index: number): string | undefined => {
    const classes: string[] = []
    // Error rows carry no verdict and draw no footprint — only a verified
    // row's stored-checksum bytes (at..frame end, per checksumSpan) light up.
    if (row !== undefined && row.error === undefined && index >= row.at && index < row.at + checksumSpan(row, frameLen)) {
      classes.push(row.ok ? 'is-ck-ok' : 'is-ck-fail')
    }
    if (attempt !== undefined) {
      if (index < attempt.offset) classes.push('is-skip')
      if (index === attempt.offset) classes.push('is-start')
    }
    return classes.length > 0 ? classes.join(' ') : undefined
  }

  const captionParts: string[] = []
  if (row !== undefined) {
    captionParts.push(
      row.error !== undefined
        ? `${row.kind}/${row.encoding} — ${row.error}`
        : `${row.kind}/${row.encoding} @ byte ${row.at} — ${row.ok ? 'match' : 'mismatch'}`,
    )
  }
  if (attempt !== undefined) {
    captionParts.push(
      attempt.offset < 0
        ? `parse @ offset ${attempt.offset} — assumes ${-attempt.offset} byte(s) before the frame`
        : attempt.offset === 0
          ? `parse @ offset 0 — ${attempt.parsed ? 'decodes' : 'fails'}`
          : `parse @ offset ${attempt.offset} — bytes 0..${attempt.offset - 1} skipped`,
    )
  }

  const adopt = useCallback((): void => {
    if (winnerRow === undefined || winnerAttempt === undefined) return
    const summary = `${winnerRow.kind} @ offset ${winnerAttempt.offset}`
    setSend({ kind: 'sending' })
    widget
      .updateModelContext({
        content: [
          {
            type: 'text',
            text: `User adopted the winning frame hypothesis from the diagnostics card: checksum ${winnerRow.kind} (${winnerRow.encoding}), parse offset ${winnerAttempt.offset} — the first probed offset that decodes structurally; the probed offsets are mutually exclusive hypotheses.`,
          },
        ],
        structuredContent: {
          kind: 'frame_hypothesis',
          checksum: winnerRow.kind,
          offset: winnerAttempt.offset,
        },
      })
      .then(() => setSend({ kind: 'sent', summary }))
      .catch((error: unknown) => {
        const detail = error instanceof Error ? error.message : String(error)
        setSend({ kind: 'failed', message: detail })
      })
  }, [widget, winnerRow, winnerAttempt])

  const toggleRow = (index: number): void => {
    setSelRow((prev) => (prev === index ? null : index))
  }
  const toggleAttempt = (index: number): void => {
    setSelAttempt((prev) => (prev === index ? null : index))
  }

  const kvParts = [
    `${frameLen} B`,
    diagnostics.command,
    diagnostics.port,
  ].filter((part): part is string => part !== undefined)

  return (
    <div className="viewer-root">
      <Card
        title="Frame diagnostics"
        head={
          <>
            {diagnostics.replay && <BadgeReplay />}
            {diagnostics.device !== undefined && <Chip>{diagnostics.device}</Chip>}
            <CardSpacer />
            <span className="live-tag">SAVED FRAME</span>
          </>
        }
        foot={
          <>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
              {resultRef.path ?? (resultRef.source === 'file' ? resultRef.uri : 'inline result')}
            </span>
            <CardSpacer />
            {resultRef.bytes !== undefined && <Chip>{resultRef.bytes} B</Chip>}
          </>
        }
      >
        <div className="ob-hex diag-hex">
          {bytes.map((byte, index) => (
            <Fragment key={index}>
              {index > 0 ? ' ' : ''}
              <i className={byteClass(index)}>{hex2(byte)}</i>
            </Fragment>
          ))}
        </div>
        <div className="viewer-kpi">
          <span>
            frame <b>{kvParts.join(' · ')}</b>
          </span>
          {captionParts.map((text) => (
            <span key={text}>{text}</span>
          ))}
        </div>

        <p className="diag-label">Checksum hypotheses · {matrix.length} tried</p>
        {matrix.length === 0 ? (
          <p className="diag-none">the tool tried no checksum hypotheses</p>
        ) : (
          <ObTable head={['checksum', 'encoding', 'at', 'result']} aria-label="Checksum hypotheses">
            {matrix.map((entry, index) => (
              <ObTableRow
                key={`${entry.kind}-${entry.encoding}`}
                selectable
                selected={selRow === index}
                onSelect={() => toggleRow(index)}
              >
                <td className={entry.ok ? 'mono diag-win' : 'mono'}>{entry.kind}</td>
                <td className="mono">{entry.encoding}</td>
                <td className="mono">{entry.at ?? '—'}</td>
                <td>
                  {entry.error !== undefined ? (
                    <span className="mono diag-detail">
                      <span className="diag-x">✗</span> {entry.error}
                    </span>
                  ) : entry.ok ? (
                    <>
                      <Chip variant="accent">match</Chip>
                      {entry.computed !== undefined && (
                        <span className="mono diag-detail"> = {entry.computed}</span>
                      )}
                    </>
                  ) : (
                    <span className="mono diag-detail">
                      <span className="diag-x">✗</span>
                      {entry.expected !== undefined && <> expected {entry.expected}</>}
                      {entry.actual !== undefined && <> · actual {entry.actual}</>}
                    </span>
                  )}
                </td>
              </ObTableRow>
            ))}
          </ObTable>
        )}

        {attempts !== undefined && (
          <>
            <p className="diag-label">Parse attempts by offset · {attempts.length} tried</p>
            <ObTable head={['offset', 'result']} aria-label="Parse attempts">
              {attempts.map((entry: ParseAttempt, index) => (
                <ObTableRow
                  key={entry.offset}
                  selectable
                  selected={selAttempt === index}
                  onSelect={() => toggleAttempt(index)}
                >
                  <td className={entry.parsed ? 'mono diag-win' : 'mono'}>{entry.offset}</td>
                  <td>
                    {entry.parsed ? (
                      <>
                        <Chip variant="accent">decodes</Chip>
                        {entry.fields !== undefined && (
                          <span className="mono diag-detail"> {fieldsText(entry.fields)}</span>
                        )}
                      </>
                    ) : (
                      <span className="mono diag-detail">
                        <span className="diag-x">✗</span> {entry.error}
                      </span>
                    )}
                  </td>
                </ObTableRow>
              ))}
            </ObTable>
          </>
        )}

        <div className="diag-actions">
          {winnerRow !== undefined && winnerAttempt !== undefined ? (
            <Btn
              variant="primary"
              disabled={send.kind === 'sending'}
              onClick={adopt}
            >
              {send.kind === 'sending'
                ? 'Adopting'
                : `Adopt ${winnerRow.kind} @ offset ${winnerAttempt.offset}`}
            </Btn>
          ) : (
            <span className="diag-none">
              {attempts === undefined
                ? 'no offset scan in this result — nothing to adopt'
                : 'no checksum match or no offset decodes — nothing to adopt'}
            </span>
          )}
          {send.kind === 'sent' && <Receipt>{send.summary} sent to the agent</Receipt>}
          <span className="tl-note">row click: highlight in hex · local only, no tool calls</span>
        </div>
        {send.kind === 'failed' && (
          <div style={{ marginTop: 10 }}>
            <ObError
              title="Hypothesis not sent"
              detail={`${send.message} — nothing reached the agent.`}
              onRetry={adopt}
            />
          </div>
        )}
      </Card>
    </div>
  )
}
