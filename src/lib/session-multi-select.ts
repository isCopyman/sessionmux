export type SessionClickIntent = "open" | "toggle" | "range"

export function sessionClickIntent(
  event:
    | {
        ctrlKey?: boolean
        metaKey?: boolean
        shiftKey?: boolean
      }
    | null
    | undefined
): SessionClickIntent {
  if (event?.shiftKey) return "range"
  if (event?.ctrlKey || event?.metaKey) return "toggle"
  return "open"
}

export function uniqueOrderedIds(ids: readonly number[]): number[] {
  const seen = new Set<number>()
  const result: number[] = []
  for (const id of ids) {
    if (seen.has(id)) continue
    seen.add(id)
    result.push(id)
  }
  return result
}

export function rangeSessionIds(
  orderedIds: readonly number[],
  fromId: number | null,
  toId: number
): number[] {
  if (fromId == null) return [toId]
  const start = orderedIds.indexOf(fromId)
  const end = orderedIds.indexOf(toId)
  if (start < 0 && end < 0) return [toId]
  if (start < 0) return [toId]
  if (end < 0) return [fromId]
  const lo = Math.min(start, end)
  const hi = Math.max(start, end)
  return orderedIds.slice(lo, hi + 1)
}

export function nextSessionSelection<T extends { id: number }>(
  current: ReadonlyMap<number, T>,
  orderedIds: readonly number[],
  lookup: (id: number) => T | undefined,
  clicked: T,
  intent: SessionClickIntent,
  anchorId: number | null
): { selected: Map<number, T>; anchorId: number | null } {
  if (intent === "open") {
    return { selected: new Map(), anchorId: clicked.id }
  }

  if (intent === "toggle") {
    const next = new Map(current)
    if (next.has(clicked.id)) next.delete(clicked.id)
    else next.set(clicked.id, clicked)
    return { selected: next, anchorId: clicked.id }
  }

  const next = new Map(current)
  for (const id of rangeSessionIds(orderedIds, anchorId, clicked.id)) {
    const item = id === clicked.id ? clicked : lookup(id)
    if (item) next.set(id, item)
  }
  return {
    selected: next,
    anchorId: anchorId ?? clicked.id,
  }
}
