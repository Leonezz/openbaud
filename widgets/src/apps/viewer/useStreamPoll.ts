// Shared stream_poll state machine for the live views (ScopeView/HeatmapView):
//
//   create subscription (session_id + parse + budgets)
//     → incremental pulls (subscription_id + since_seq = last seq + 1)
//     → on failure: visible backoff retry; after 3 straight failures the
//       subscription is abandoned and re-created (also visible)
//
// The subscription is per-consumer — this widget never shares the agent's
// subscription, so it can never steal frames from it. Polling pauses while
// document.hidden (harness tab switches; a real host unmounts hidden widgets
// anyway) and resumes on visibility. Nothing here re-decodes bytes: frames
// arrive already parsed by the server (parse_with_spec), and this hook only
// validates the page shape (stream.ts) and counts honestly — every failure,
// retry and dropped-frame total is surfaced in the returned status.
//
// No explicit close on unmount: there is no UI left to surface a failed close,
// and the server sweeps idle subscriptions after its 120 s TTL by design.
import { useEffect, useRef, useState } from 'react'
import type { WidgetHandle } from '../../mcp/useWidget'
import {
  readStreamPage,
  type StreamFrame,
  type StreamParseRef,
  type StreamRef,
} from './stream'

export const POLL_INTERVAL_MS = 300
const POLL_MAX_FRAMES = 64
const POLL_MAX_INLINE_BYTES = 4096
/** Consecutive failures on one subscription before it is re-created. */
const FAILURES_BEFORE_RESUBSCRIBE = 3
const RETRY_BASE_MS = 500
const RETRY_CAP_MS = 5000

export type StreamPhase = 'connecting' | 'live' | 'retrying' | 'paused'

export interface StreamStatus {
  readonly phase: StreamPhase
  readonly subscriptionId: string | undefined
  /** Last failure message — stays visible for the whole retry cycle. */
  readonly error: string | undefined
  /** Delay before the pending retry fires (set while phase is 'retrying'). */
  readonly retryDelayMs: number | undefined
  /** Consecutive failures so far (0 while healthy). */
  readonly attempt: number
  /** Successful polls (including empty ones). */
  readonly polls: number
  /** Frames delivered to the consumer so far. */
  readonly framesDelivered: number
  /** Frames that arrived with a parse_error — counted, never hidden. */
  readonly parseErrors: number
  /** Server-side loss totals for this subscription (cumulative). */
  readonly droppedFrames: number
  readonly droppedChunks: number
  /** ts_ms of the newest delivered frame. */
  readonly lastTsMs: number | undefined
  /** parsed-field name → unit string (run_command units mechanism). */
  readonly units: Readonly<Record<string, string>>
  /** The server's parse echo — proof parse is actually in effect. */
  readonly parseEcho: StreamParseRef | undefined
}

const INITIAL_STATUS: StreamStatus = {
  phase: 'connecting',
  subscriptionId: undefined,
  error: undefined,
  retryDelayMs: undefined,
  attempt: 0,
  polls: 0,
  framesDelivered: 0,
  parseErrors: 0,
  droppedFrames: 0,
  droppedChunks: 0,
  lastTsMs: undefined,
  units: {},
  parseEcho: undefined,
}

function retryDelay(attempt: number): number {
  return Math.min(RETRY_CAP_MS, RETRY_BASE_MS * 2 ** Math.max(0, attempt - 1))
}

function errorTextOf(result: { content: readonly { type: string; text?: string }[] }): string {
  const texts = result.content.flatMap((block) =>
    block.type === 'text' && typeof block.text === 'string' ? [block.text] : [],
  )
  const joined = texts.join('\n').trim()
  return joined !== '' ? joined : 'stream_poll failed (host returned no error text)'
}

export interface FrameDelivery {
  /**
   * True when these frames come from a different subscription than the last
   * delivery (the old one was abandoned and re-created): seq numbering and
   * the ts_ms base may have restarted, so the consumer must reset any buffer
   * keyed on them instead of mixing two time bases into one trace.
   */
  readonly reset: boolean
}

/**
 * Runs the poll loop for one stream descriptor and reports every state
 * transition through the returned status. New frames are handed to `onFrames`
 * in delivery order; the consumer owns its own buffer (ring for the scope,
 * latest-grid for the heatmap).
 */
export function useStreamPoll(
  widget: WidgetHandle,
  stream: StreamRef,
  onFrames: (frames: readonly StreamFrame[], delivery: FrameDelivery) => void,
): StreamStatus {
  const [status, setStatus] = useState<StreamStatus>(INITIAL_STATUS)
  const onFramesRef = useRef(onFrames)
  useEffect(() => {
    onFramesRef.current = onFrames
  }, [onFrames])

  const { callTool } = widget
  const { sessionId } = stream
  const { device, command } = stream.parse

  useEffect(() => {
    let cancelled = false
    let timer: number | undefined
    let subscriptionId: string | undefined
    let sinceSeq: number | undefined
    let failures = 0
    let inFlight = false
    /** Subscription id the last delivered frames belonged to. */
    let deliveredFrom: string | undefined
    setStatus(INITIAL_STATUS)

    const schedule = (delayMs: number): void => {
      if (cancelled) return
      timer = window.setTimeout(() => void poll(), delayMs)
    }

    const poll = async (): Promise<void> => {
      // The visibility handler can fire while a poll is awaiting its result;
      // never run two polls at once (double delivery, out-of-order acks).
      if (cancelled || inFlight) return
      if (document.hidden) {
        // Paused, not dead: visibilitychange below restarts the loop.
        setStatus((prev) => ({ ...prev, phase: 'paused', retryDelayMs: undefined }))
        return
      }
      const args: Record<string, unknown> =
        subscriptionId === undefined
          ? {
              session_id: sessionId,
              parse: { device, command },
              max_frames: POLL_MAX_FRAMES,
              max_inline_bytes: POLL_MAX_INLINE_BYTES,
            }
          : {
              subscription_id: subscriptionId,
              ...(sinceSeq !== undefined ? { since_seq: sinceSeq } : {}),
              max_frames: POLL_MAX_FRAMES,
              max_inline_bytes: POLL_MAX_INLINE_BYTES,
            }
      inFlight = true
      try {
        const result = await callTool('stream_poll', args)
        if (cancelled) return
        if (result.isError) throw new Error(errorTextOf(result))
        const read = readStreamPage(result.structuredContent)
        if (read.kind === 'invalid') throw new Error(read.reason)
        const page = read.page
        subscriptionId = page.subscriptionId
        failures = 0
        const last = page.frames[page.frames.length - 1]
        if (last !== undefined) sinceSeq = last.seq + 1
        const parseErrors = page.frames.filter((f) => f.parseError !== undefined).length
        setStatus((prev) => ({
          ...prev,
          phase: 'live',
          subscriptionId: page.subscriptionId,
          error: undefined,
          retryDelayMs: undefined,
          attempt: 0,
          polls: prev.polls + 1,
          framesDelivered: prev.framesDelivered + page.frames.length,
          parseErrors: prev.parseErrors + parseErrors,
          droppedFrames: page.droppedFrames,
          droppedChunks: page.droppedChunks,
          lastTsMs: last?.tsMs ?? prev.lastTsMs,
          units: Object.keys(page.units).length > 0 ? page.units : prev.units,
          parseEcho: page.parse ?? prev.parseEcho,
        }))
        if (page.frames.length > 0) {
          const reset = deliveredFrom !== undefined && deliveredFrom !== page.subscriptionId
          deliveredFrom = page.subscriptionId
          onFramesRef.current(page.frames, { reset })
        }
        schedule(POLL_INTERVAL_MS)
      } catch (error) {
        if (cancelled) return
        failures += 1
        const message = error instanceof Error ? error.message : String(error)
        if (subscriptionId !== undefined && failures >= FAILURES_BEFORE_RESUBSCRIBE) {
          // The subscription may be gone (TTL sweep, server restart) —
          // abandon it and re-create, visibly: subscriptionId clears and the
          // phase returns to 'connecting' on the next attempt.
          subscriptionId = undefined
          sinceSeq = undefined
        }
        const delayMs = retryDelay(failures)
        setStatus((prev) => ({
          ...prev,
          phase: 'retrying',
          subscriptionId,
          error: message,
          retryDelayMs: delayMs,
          attempt: failures,
        }))
        schedule(delayMs)
      } finally {
        inFlight = false
      }
    }

    const onVisibility = (): void => {
      if (cancelled || document.hidden) return
      // Coming back from 'paused': the loop stopped scheduling, restart it.
      if (timer !== undefined) window.clearTimeout(timer)
      void poll()
    }
    document.addEventListener('visibilitychange', onVisibility)
    void poll()

    return () => {
      cancelled = true
      document.removeEventListener('visibilitychange', onVisibility)
      if (timer !== undefined) window.clearTimeout(timer)
    }
  }, [callTool, sessionId, device, command])

  return status
}
