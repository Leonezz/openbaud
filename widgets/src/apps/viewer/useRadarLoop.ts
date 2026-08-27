import { useCallback, useEffect, useRef, useState } from 'react'

export const RATE_OPTIONS = [1, 2, 5] as const
export type RateHz = (typeof RATE_OPTIONS)[number]

export interface RadarLoop {
  readonly playing: boolean
  readonly rateHz: RateHz
  readonly setRate: (rate: RateHz) => void
  readonly toggle: () => void
  readonly resume: () => void
}

export interface RadarLoopOptions {
  /** Whether the originating tool call is known (name + arguments). */
  readonly canPoll: boolean
  /** Under prefers-reduced-motion the loop never starts on its own. */
  readonly reducedMotion: boolean
  /** One poll; resolve false to pause the loop (failure already surfaced). */
  readonly poll: () => Promise<boolean>
}

/** Continuous-scan driver: setInterval at 1/2/5 Hz, one call in flight max. */
export function useRadarLoop({ canPoll, reducedMotion, poll }: RadarLoopOptions): RadarLoop {
  const [playing, setPlaying] = useState(false)
  const [rateHz, setRateHz] = useState<RateHz>(2)
  // Once the user (or a failure) pauses, auto-start must not fight them.
  const userPausedRef = useRef(false)
  const pollRef = useRef(poll)
  useEffect(() => {
    pollRef.current = poll
  }, [poll])

  useEffect(() => {
    if (!canPoll) {
      setPlaying(false)
      return
    }
    if (!reducedMotion && !userPausedRef.current) setPlaying(true)
  }, [canPoll, reducedMotion])

  useEffect(() => {
    if (!playing) return
    let cancelled = false
    let inFlight = false
    const tick = (): void => {
      if (inFlight) return
      inFlight = true
      void pollRef.current().then((ok) => {
        inFlight = false
        if (!ok && !cancelled) {
          userPausedRef.current = true
          setPlaying(false)
        }
      })
    }
    const id = window.setInterval(tick, 1000 / rateHz)
    return () => {
      cancelled = true
      window.clearInterval(id)
    }
  }, [playing, rateHz])

  const toggle = useCallback(() => {
    setPlaying((was) => {
      userPausedRef.current = was
      return !was
    })
  }, [])

  const resume = useCallback(() => {
    userPausedRef.current = false
    setPlaying(true)
  }, [])

  const setRate = useCallback((rate: RateHz) => setRateHz(rate), [])

  return { playing, rateHz, setRate, toggle, resume }
}
