import { Btn } from '../../components/Btn'
import { CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { Receipt } from '../../components/Receipt'

/** The picker only reports a choice; opening stays the agent's call. */
export type SendState =
  | { readonly kind: 'idle' }
  | { readonly kind: 'sending'; readonly path: string }
  | { readonly kind: 'failed'; readonly path: string; readonly message: string }
  | { readonly kind: 'sent'; readonly path: string }

function CandidateCount({ count }: { count: number }) {
  return (
    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
      {count === 1 ? '1 candidate' : `${count} candidates`}
    </span>
  )
}

function SendStatus({ send }: { send: SendState }) {
  switch (send.kind) {
    case 'idle':
      return <span>Your pick goes to the agent — it decides when to open the port.</span>
    case 'sending':
      return <span>Sending the selection to the agent …</span>
    case 'failed':
      return <span>Selection was not sent — see the error above.</span>
    case 'sent':
      return (
        <>
          <Receipt>Sent to the agent</Receipt>
          <Chip>{send.path}</Chip>
        </>
      )
  }
}

export interface PickerFootProps {
  count: number
  send: SendState
  canSend: boolean
  onSend: () => void
  onRescan: () => void
}

export function PickerFoot({ count, send, canSend, onSend, onRescan }: PickerFootProps) {
  const sending = send.kind === 'sending'
  return (
    <>
      <SendStatus send={send} />
      <CardSpacer />
      <CandidateCount count={count} />
      <Btn variant="ghost" disabled={sending} onClick={onRescan}>
        Rescan
      </Btn>
      {sending ? (
        // pp-loading (pointer-events: none), not [disabled]: the primary look
        // stays put while the spinner communicates busy.
        <Btn variant="primary" className="pp-loading">
          <span className="pp-spin" aria-hidden="true" />
          Sending…
        </Btn>
      ) : (
        <Btn variant="primary" disabled={!canSend} onClick={onSend}>
          Use this port
        </Btn>
      )}
    </>
  )
}

export interface RescanFootProps {
  /** Candidate count when known (0 for the empty state); omitted after a failure. */
  count?: number
  onRescan: () => void
}

export function RescanFoot({ count, onRescan }: RescanFootProps) {
  return (
    <>
      <CardSpacer />
      {count !== undefined && <CandidateCount count={count} />}
      <Btn onClick={onRescan}>Rescan</Btn>
    </>
  )
}

export function LoadingFoot() {
  return (
    <>
      <span className="ob-loading" style={{ width: 150 }} />
      <CardSpacer />
      <span className="ob-loading" style={{ width: 76 }} />
    </>
  )
}
