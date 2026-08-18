import type { SessionTimer } from "@/lib/types"

/**
 * Frontend mirror of the backend's pure-backoff schedule (see
 * `src-tauri/src/session_timer.rs`): the reminder delay is
 * `grace × 2^strike`, capped at 30 minutes. The backend is the only trigger
 * authority — nothing here schedules anything; these helpers only ESTIMATE the
 * next fire so the collapsed pill can say what it is waiting for.
 */
export const MAX_REMINDER_DELAY_SECS = 30 * 60

export function reminderDelaySecs(graceSecs: number, strike: number): number {
  const shift = Math.min(Math.max(Math.trunc(strike), 0), 20)
  return Math.min(graceSecs * 2 ** shift, MAX_REMINDER_DELAY_SECS)
}

/**
 * Estimated next-fire instant (epoch ms) for one timer, or null when the timer
 * is paused. The backend counts from the conversation's last idle transition,
 * which the frontend cannot see, so the base is the last fire (failing that,
 * the last edit/reset) — hence "estimate": display it with a `~`.
 */
export function nextFireEstimate(
  timer: Pick<
    SessionTimer,
    "enabled" | "idleGraceSecs" | "strikeCount" | "lastFiredAt" | "updatedAt"
  >
): number | null {
  if (!timer.enabled) return null
  const baseRaw = timer.lastFiredAt ?? timer.updatedAt
  const baseMs = Date.parse(baseRaw)
  if (Number.isNaN(baseMs)) return null
  return (
    baseMs + reminderDelaySecs(timer.idleGraceSecs, timer.strikeCount) * 1000
  )
}

/** The enabled timer that fires next, with its estimated instant. */
export function nextFireTimer(
  timers: readonly SessionTimer[]
): { timer: SessionTimer; at: number } | null {
  let best: { timer: SessionTimer; at: number } | null = null
  for (const timer of timers) {
    const at = nextFireEstimate(timer)
    if (at == null) continue
    if (!best || at < best.at) best = { timer, at }
  }
  return best
}

/**
 * Compact countdown for a pill: "45s", "3m", "1h5m". Rounds up sub-minute
 * remainders so the badge never reads "0s" while still pending.
 */
export function formatCompactCountdown(remainingMs: number): string {
  const secs = Math.ceil(remainingMs / 1000)
  if (secs < 60) return `${Math.max(1, secs)}s`
  const mins = Math.floor(secs / 60)
  if (mins < 60) return `${mins}m`
  const hours = Math.floor(mins / 60)
  const restMins = mins % 60
  return restMins > 0 ? `${hours}h${restMins}m` : `${hours}h`
}
