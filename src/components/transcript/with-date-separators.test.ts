import { describe, expect, it } from "vitest"

import {
  calendarDayKey,
  formatTranscriptDayLabel,
  withDateSeparators,
} from "./with-date-separators"

function localMs(year: number, month: number, day: number, hour = 12): number {
  return new Date(year, month - 1, day, hour, 0, 0, 0).getTime()
}

type Row = { id: string; at: number | null }

function getTimeMs(row: Row): number | null {
  return row.at
}

function ymd(timeMs: number): string {
  const key = calendarDayKey(timeMs, "local")
  return key ?? "invalid"
}

describe("withDateSeparators", () => {
  it("returns an empty stream for an empty input", () => {
    expect(withDateSeparators([], getTimeMs, ymd)).toEqual([])
  })

  it("wraps a single valid item in one separator plus the item", () => {
    const items: Row[] = [{ id: "a", at: localMs(2026, 8, 21) }]
    expect(withDateSeparators(items, getTimeMs, ymd)).toEqual([
      { kind: "separator", label: "2026-08-21", key: "date-2026-08-21" },
      { kind: "item", item: items[0], index: 0 },
    ])
  })

  it("shares one separator across several items on the same local day", () => {
    const items: Row[] = [
      { id: "a", at: localMs(2026, 8, 21, 9) },
      { id: "b", at: localMs(2026, 8, 21, 18) },
    ]
    const stream = withDateSeparators(items, getTimeMs, ymd)
    expect(stream.filter((e) => e.kind === "separator")).toHaveLength(1)
    expect(stream).toEqual([
      { kind: "separator", label: "2026-08-21", key: "date-2026-08-21" },
      { kind: "item", item: items[0], index: 0 },
      { kind: "item", item: items[1], index: 1 },
    ])
  })

  it("inserts a new separator when the local calendar day changes", () => {
    const items: Row[] = [
      { id: "a", at: localMs(2026, 8, 20, 23) },
      { id: "b", at: localMs(2026, 8, 21, 0) },
    ]
    const stream = withDateSeparators(items, getTimeMs, ymd)
    expect(stream.map((e) => e.kind)).toEqual([
      "separator",
      "item",
      "separator",
      "item",
    ])
    expect(
      stream.filter((e) => e.kind === "separator").map((e) => e.label)
    ).toEqual(["2026-08-20", "2026-08-21"])
  })

  it("treats a year boundary as a new day", () => {
    const items: Row[] = [
      { id: "nye", at: localMs(2025, 12, 31, 23) },
      { id: "nyd", at: localMs(2026, 1, 1, 0) },
    ]
    const labels = withDateSeparators(items, getTimeMs, ymd)
      .filter((e) => e.kind === "separator")
      .map((e) => e.label)
    expect(labels).toEqual(["2025-12-31", "2026-01-01"])
  })

  it("emits items with invalid timestamps and does not insert a separator for them", () => {
    const items: Row[] = [
      { id: "bad-null", at: null },
      { id: "bad-nan", at: Number.NaN },
      { id: "ok", at: localMs(2026, 8, 21) },
      { id: "bad-inf", at: Number.POSITIVE_INFINITY },
    ]
    const stream = withDateSeparators(items, getTimeMs, ymd)
    expect(stream).toEqual([
      { kind: "item", item: items[0], index: 0 },
      { kind: "item", item: items[1], index: 1 },
      { kind: "separator", label: "2026-08-21", key: "date-2026-08-21" },
      { kind: "item", item: items[2], index: 2 },
      { kind: "item", item: items[3], index: 3 },
    ])
  })

  it("emits only items when every timestamp is invalid", () => {
    const items: Row[] = [
      { id: "a", at: null },
      { id: "b", at: Number.NaN },
    ]
    expect(withDateSeparators(items, getTimeMs, ymd)).toEqual([
      { kind: "item", item: items[0], index: 0 },
      { kind: "item", item: items[1], index: 1 },
    ])
  })

  it("does not sort: a later return to an earlier day inserts another separator", () => {
    const items: Row[] = [
      { id: "a", at: localMs(2026, 8, 21) },
      { id: "b", at: localMs(2026, 8, 20) },
      { id: "c", at: localMs(2026, 8, 21) },
    ]
    const stream = withDateSeparators(items, getTimeMs, ymd)
    const separators = stream.filter((e) => e.kind === "separator")
    expect(separators.map((e) => e.label)).toEqual([
      "2026-08-21",
      "2026-08-20",
      "2026-08-21",
    ])
    expect(separators.map((e) => e.key)).toEqual([
      "date-2026-08-21",
      "date-2026-08-20",
      "date-2026-08-21-2",
    ])
    expect(stream.filter((e) => e.kind === "item").map((e) => e.index)).toEqual(
      [0, 1, 2]
    )
  })

  it("can group by UTC day independently of the host timezone", () => {
    // 2026-08-21T01:00:00.000Z — always 21 Aug in UTC.
    const utcMorning = Date.UTC(2026, 7, 21, 1, 0, 0)
    const items: Row[] = [{ id: "a", at: utcMorning }]
    const utcLabel = (ms: number) => calendarDayKey(ms, "utc") ?? ""
    const stream = withDateSeparators(items, getTimeMs, utcLabel, {
      timeZone: "utc",
    })
    expect(stream[0]).toEqual({
      kind: "separator",
      label: "2026-08-21",
      key: "date-2026-08-21",
    })
  })
})

describe("formatTranscriptDayLabel", () => {
  it("returns Today / Yesterday relative to nowMs in local time", () => {
    const now = localMs(2026, 8, 21, 15)
    expect(
      formatTranscriptDayLabel(localMs(2026, 8, 21, 9), { nowMs: now })
    ).toBe("Today")
    expect(
      formatTranscriptDayLabel(localMs(2026, 8, 20, 9), { nowMs: now })
    ).toBe("Yesterday")
  })

  it("formats older dates with Intl", () => {
    const now = localMs(2026, 8, 21)
    const label = formatTranscriptDayLabel(localMs(2025, 1, 2), {
      nowMs: now,
      locale: "en",
    })
    expect(label).toMatch(/2025/)
  })
})
