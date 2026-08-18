import { describe, expect, it } from "vitest"

import {
  formatCompactCountdown,
  MAX_REMINDER_DELAY_SECS,
  nextFireEstimate,
  nextFireTimer,
  reminderDelaySecs,
} from "./session-timer-next-fire"
import type { SessionTimer } from "./types"

function timer(overrides: Partial<SessionTimer> = {}): SessionTimer {
  return {
    id: "t1",
    conversationId: 7,
    idleGraceSecs: 60,
    promptText: "continue",
    enabled: true,
    lastFiredAt: null,
    fireCount: 0,
    strikeCount: 0,
    createdAt: "2026-08-19T11:00:00Z",
    updatedAt: "2026-08-19T11:00:00Z",
    ...overrides,
  }
}

describe("reminderDelaySecs", () => {
  it("doubles per strike and caps at 30 minutes", () => {
    expect(reminderDelaySecs(2, 0)).toBe(2)
    expect(reminderDelaySecs(2, 3)).toBe(16)
    expect(reminderDelaySecs(60, 10)).toBe(MAX_REMINDER_DELAY_SECS)
    // A negative or absurd strike can't blow up the shift.
    expect(reminderDelaySecs(60, -2)).toBe(60)
    expect(reminderDelaySecs(60, 100)).toBe(MAX_REMINDER_DELAY_SECS)
  })
})

describe("nextFireEstimate", () => {
  it("is null for a paused timer", () => {
    expect(nextFireEstimate(timer({ enabled: false }))).toBeNull()
  })

  it("counts from the last fire when there is one", () => {
    const at = nextFireEstimate(
      timer({
        lastFiredAt: "2026-08-19T11:50:00Z",
        strikeCount: 1, // 60s × 2 = 2m
      })
    )
    expect(at).toBe(Date.parse("2026-08-19T11:52:00Z"))
  })

  it("falls back to the last edit/reset for a never-fired timer", () => {
    const at = nextFireEstimate(timer())
    expect(at).toBe(Date.parse("2026-08-19T11:01:00Z"))
  })

  it("is null when the base timestamp is unparseable", () => {
    expect(nextFireEstimate(timer({ updatedAt: "not-a-date" }))).toBeNull()
  })
})

describe("nextFireTimer", () => {
  it("picks the enabled timer that fires soonest", () => {
    const soon = timer({
      id: "soon",
      lastFiredAt: "2026-08-19T11:59:00Z",
      idleGraceSecs: 120, // → 12:01
    })
    const later = timer({
      id: "later",
      lastFiredAt: "2026-08-19T11:59:00Z",
      idleGraceSecs: 600, // → 12:09
    })
    const paused = timer({ id: "paused", enabled: false })
    expect(nextFireTimer([later, paused, soon])?.timer.id).toBe("soon")
    expect(nextFireTimer([paused])).toBeNull()
    expect(nextFireTimer([])).toBeNull()
  })
})

describe("formatCompactCountdown", () => {
  it("formats seconds, minutes and hours compactly", () => {
    expect(formatCompactCountdown(1)).toBe("1s")
    expect(formatCompactCountdown(45_000)).toBe("45s")
    // Sub-minute remainders round up, tipping 59.5s into the minute branch.
    expect(formatCompactCountdown(59_500)).toBe("1m")
    expect(formatCompactCountdown(60_000)).toBe("1m")
    expect(formatCompactCountdown(3 * 60_000)).toBe("3m")
    expect(formatCompactCountdown(59 * 60_000)).toBe("59m")
    expect(formatCompactCountdown(60 * 60_000)).toBe("1h")
    expect(formatCompactCountdown(65 * 60_000)).toBe("1h5m")
  })
})
