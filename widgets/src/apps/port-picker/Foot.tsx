import { Btn } from '../../components/Btn'
import { CardSpacer } from '../../components/Card'
import { Chip } from '../../components/Chip'
import { Receipt } from '../../components/Receipt'
import type { OpenedSession } from './ports'

function CandidateCount({ count }: { count: number }) {
  return (
    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
      {count === 1 ? '1 candidate' : `${count} candidates`}
    </span>
  )
}

export interface IdleFootProps {
  count: number
  canOpen: boolean
  onOpen: () => void
}

export function IdleFoot({ count, canOpen, onOpen }: IdleFootProps) {
  return (
    <>
      <span>
        {count === 1 ? 'Preselected — waiting for confirmation · ' : 'Open at '}
        <b>115200 8N1</b> (default)
      </span>
      <CardSpacer />
      <CandidateCount count={count} />
      <Btn variant="primary" disabled={!canOpen} onClick={onOpen}>
        Open port
      </Btn>
    </>
  )
}

export function OpeningFoot({ port }: { port: string }) {
  return (
    <>
      <span>Opening {port} …</span>
      <CardSpacer />
      {/* pp-loading (pointer-events none), not [disabled]: the design keeps
          the primary look while the spinner communicates busy. */}
      <Btn variant="primary" className="pp-loading">
        <span className="pp-spin" aria-hidden="true" />
        Opening…
      </Btn>
    </>
  )
}

export interface FailedFootProps {
  onReselect: () => void
  onRetry: () => void
}

export function FailedFoot({ onReselect, onRetry }: FailedFootProps) {
  return (
    <>
      <span>Open failed — nothing was opened</span>
      <CardSpacer />
      <Btn variant="ghost" onClick={onReselect}>
        Choose another port
      </Btn>
      <Btn variant="primary" onClick={onRetry}>
        Retry
      </Btn>
    </>
  )
}

export function OpenedFoot({ session }: { session: OpenedSession }) {
  return (
    <>
      <Receipt>Session opened</Receipt>
      <CardSpacer />
      <Chip>session: {session.sessionId}</Chip>
      <Chip>{session.baud} 8N1</Chip>
    </>
  )
}

export interface RescanFootProps {
  /** Candidate count when known (0 for the empty state); omitted after a scan failure. */
  count?: number
  onUseMock: () => void
  onRescan: () => void
}

export function RescanFoot({ count, onUseMock, onRescan }: RescanFootProps) {
  return (
    <>
      <CardSpacer />
      {count !== undefined && <CandidateCount count={count} />}
      <Btn variant="ghost" onClick={onUseMock}>
        Use mock:echo
      </Btn>
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
