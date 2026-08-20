/**
 * Insert calendar-day separators into a transcript-like item stream.
 *
 * Proposed contracts (not silently decided — see the charter-1 report):
 * - Timezone: default `"local"`. Pass `{ timeZone: "utc" }` to group by UTC
 *   date. Room/session callers should pick one and stick to it.
 * - Order: **do not sort**. Walk `items` as given. A separator is inserted
 *   when the calendar day of the current *valid* timestamp differs from the
 *   previous *valid* timestamp. Unsorted input can repeat the same label.
 */

export type TranscriptDateTimeZone = "local" | "utc"

export type TranscriptDateSeparatorEntry = {
  kind: "separator"
  label: string
  /** Stable virtualization key: `date-<yyyy-mm-dd>` plus a disambiguator
   *  when the same calendar day appears again (unsorted input). */
  key: string
}

export type TranscriptDateItemEntry<T> = {
  kind: "item"
  item: T
  /** Index in the original `items` array, not the mixed stream. */
  index: number
}

export type TranscriptDatedEntry<T> =
  | TranscriptDateItemEntry<T>
  | TranscriptDateSeparatorEntry

export type WithDateSeparatorsOptions = {
  timeZone?: TranscriptDateTimeZone
}

export function withDateSeparators<T>(
  items: readonly T[],
  getTimeMs: (item: T) => number | null | undefined,
  formatLabel: (timeMs: number) => string,
  options?: WithDateSeparatorsOptions
): TranscriptDatedEntry<T>[] {
  const timeZone = options?.timeZone ?? "local"
  const out: TranscriptDatedEntry<T>[] = []
  let previousDayKey: string | null = null
  const dayOccurrence = new Map<string, number>()

  for (let index = 0; index < items.length; index++) {
    const item = items[index] as T
    const timeMs = getTimeMs(item)
    const dayKey = calendarDayKey(timeMs, timeZone)

    if (dayKey !== null && dayKey !== previousDayKey) {
      const occurrence = (dayOccurrence.get(dayKey) ?? 0) + 1
      dayOccurrence.set(dayKey, occurrence)
      out.push({
        kind: "separator",
        label: formatLabel(timeMs as number),
        key:
          occurrence === 1 ? `date-${dayKey}` : `date-${dayKey}-${occurrence}`,
      })
      previousDayKey = dayKey
    }

    out.push({ kind: "item", item, index })
  }

  return out
}

export function calendarDayKey(
  timeMs: number | null | undefined,
  timeZone: TranscriptDateTimeZone = "local"
): string | null {
  if (typeof timeMs !== "number" || !Number.isFinite(timeMs)) return null
  const date = new Date(timeMs)
  if (Number.isNaN(date.getTime())) return null
  if (timeZone === "utc") {
    return `${date.getUTCFullYear()}-${pad2(date.getUTCMonth() + 1)}-${pad2(date.getUTCDate())}`
  }
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`
}

function pad2(value: number): string {
  return String(value).padStart(2, "0")
}

/**
 * Minimal day label for callers that have not wired next-intl yet.
 * Uses `Intl.DateTimeFormat` so older dates follow the given locale without
 * importing next-intl. "Today" / "Yesterday" stay English unless the caller
 * injects `todayLabel` / `yesterdayLabel` (Room wires these from i18n).
 */
export function formatTranscriptDayLabel(
  timeMs: number,
  options?: {
    nowMs?: number
    locale?: string
    timeZone?: TranscriptDateTimeZone
    todayLabel?: string
    yesterdayLabel?: string
  }
): string {
  const timeZone = options?.timeZone ?? "local"
  const nowMs = options?.nowMs ?? Date.now()
  const day = calendarDayKey(timeMs, timeZone)
  const today = calendarDayKey(nowMs, timeZone)
  const yesterday = calendarDayKey(nowMs - 24 * 60 * 60 * 1000, timeZone)
  if (day !== null && day === today) return options?.todayLabel ?? "Today"
  if (day !== null && day === yesterday)
    return options?.yesterdayLabel ?? "Yesterday"
  const date = new Date(timeMs)
  const locale = options?.locale ?? "en"
  return new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    timeZone: timeZone === "utc" ? "UTC" : undefined,
  }).format(date)
}
