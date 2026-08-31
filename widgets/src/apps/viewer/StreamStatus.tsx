// Shared status strip for the live views: the poll state machine made
// visible. Every phase, retry, failure message and loss counter is shown as
// text next to an LED — never color alone, never a hidden failure.
import { Led, type LedTone } from '../../components/Led'
import { ObError } from '../../components/ObError'
import type { StreamStatus } from './useStreamPoll'

function phaseLabel(status: StreamStatus): { tone: LedTone; pulse: boolean; text: string } {
  switch (status.phase) {
    case 'connecting':
      return { tone: 'warn', pulse: true, text: 'connecting — creating subscription' }
    case 'live':
      return { tone: 'ok', pulse: true, text: 'LIVE' }
    case 'paused':
      return { tone: 'off', pulse: false, text: 'paused — tab hidden' }
    case 'retrying':
      return {
        tone: 'err',
        pulse: true,
        text: `retrying in ${status.retryDelayMs ?? 0} ms — attempt ${status.attempt}`,
      }
  }
}

/** KPI row: phase, subscription, delivery counters, loss counters. */
export function StreamStatusLine({ status }: { readonly status: StreamStatus }) {
  const phase = phaseLabel(status)
  const lossy = status.droppedFrames > 0 || status.droppedChunks > 0
  return (
    <div className="viewer-kpi stream-status">
      <span className="stream-phase">
        <Led tone={phase.tone} pulse={phase.pulse} />
        <b>{phase.text}</b>
      </span>
      <span>
        sub <b>{status.subscriptionId ?? 'none yet'}</b>
      </span>
      <span>
        frames <b>{status.framesDelivered}</b> in <b>{status.polls}</b> polls
      </span>
      <span className={status.parseErrors > 0 ? 'stream-warn' : undefined}>
        parse errors <b>{status.parseErrors}</b>
      </span>
      {lossy && (
        <span className="stream-loss">
          <Led tone="err" />
          <b>
            dropped {status.droppedFrames} frames / {status.droppedChunks} chunks
          </b>
        </span>
      )}
    </div>
  )
}

/** The last failure, kept on screen for the whole retry cycle. */
export function StreamErrorNotice({ status }: { readonly status: StreamStatus }) {
  if (status.error === undefined) return null
  return (
    <div style={{ marginTop: 10 }}>
      <ObError
        title="stream_poll failed — retrying"
        detail={`${status.error} (attempt ${status.attempt}, next try in ${status.retryDelayMs ?? 0} ms)`}
      />
    </div>
  )
}
