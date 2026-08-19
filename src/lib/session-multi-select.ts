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

export function uniqueOrderedKeys<K>(keys: readonly K[]): K[] {
  const seen = new Set<K>()
  const result: K[] = []
  for (const key of keys) {
    if (seen.has(key)) continue
    seen.add(key)
    result.push(key)
  }
  return result
}

export function uniqueOrderedIds(ids: readonly number[]): number[] {
  return uniqueOrderedKeys(ids)
}

export function rangeKeys<K>(
  orderedKeys: readonly K[],
  fromKey: K | null,
  toKey: K
): K[] {
  if (fromKey == null) return [toKey]
  const start = orderedKeys.indexOf(fromKey)
  const end = orderedKeys.indexOf(toKey)
  if (start < 0 && end < 0) return [toKey]
  if (start < 0) return [toKey]
  if (end < 0) return [fromKey]
  const lo = Math.min(start, end)
  const hi = Math.max(start, end)
  return orderedKeys.slice(lo, hi + 1)
}

export function rangeSessionIds(
  orderedIds: readonly number[],
  fromId: number | null,
  toId: number
): number[] {
  return rangeKeys(orderedIds, fromId, toId)
}

/**
 * Key-generic selection core: same open/toggle/range semantics as
 * {@link nextSessionSelection}, but the caller names the clicked key
 * explicitly, so one selection can mix item kinds (Sessions and Rooms share
 * the sidebar selection; their id spaces differ).
 */
export function nextKeySelection<K, T>(args: {
  current: ReadonlyMap<K, T>
  orderedKeys: readonly K[]
  lookup: (key: K) => T | undefined
  clickedKey: K
  clicked: T
  intent: SessionClickIntent
  anchorKey: K | null
}): { selected: Map<K, T>; anchorKey: K | null } {
  const {
    current,
    orderedKeys,
    lookup,
    clickedKey,
    clicked,
    intent,
    anchorKey,
  } = args
  if (intent === "open") {
    return { selected: new Map(), anchorKey: clickedKey }
  }

  if (intent === "toggle") {
    const next = new Map(current)
    if (next.has(clickedKey)) next.delete(clickedKey)
    else next.set(clickedKey, clicked)
    return { selected: next, anchorKey: clickedKey }
  }

  const next = new Map(current)
  for (const key of rangeKeys(orderedKeys, anchorKey, clickedKey)) {
    const item = key === clickedKey ? clicked : lookup(key)
    if (item) next.set(key, item)
  }
  return { selected: next, anchorKey: anchorKey ?? clickedKey }
}

export function nextSessionSelection<T extends { id: number }>(
  current: ReadonlyMap<number, T>,
  orderedIds: readonly number[],
  lookup: (id: number) => T | undefined,
  clicked: T,
  intent: SessionClickIntent,
  anchorId: number | null
): { selected: Map<number, T>; anchorId: number | null } {
  const next = nextKeySelection({
    current,
    orderedKeys: orderedIds,
    lookup,
    clickedKey: clicked.id,
    clicked,
    intent,
    anchorKey: anchorId,
  })
  return { selected: next.selected, anchorId: next.anchorKey }
}
