export interface ConversationFindEntry {
  itemKey: string
  threadIndex: number
  text: string
}

export interface ConversationFindMatch {
  id: string
  itemKey: string
  threadIndex: number
  occurrenceIndex: number
  start: number
  end: number
}

/** Literal, case-insensitive search over the semantic text of transcript rows. */
export function findConversationMatches(
  entries: readonly ConversationFindEntry[],
  query: string
): ConversationFindMatch[] {
  if (query.length === 0) return []
  const needle = query.toLocaleLowerCase()
  const matches: ConversationFindMatch[] = []

  for (const entry of entries) {
    const haystack = entry.text.toLocaleLowerCase()
    let occurrenceIndex = 0
    let fromIndex = 0
    while (fromIndex <= haystack.length) {
      const start = haystack.indexOf(needle, fromIndex)
      if (start < 0) break
      const end = start + query.length
      matches.push({
        id: `${entry.itemKey}:${occurrenceIndex}:${start}:${end}`,
        itemKey: entry.itemKey,
        threadIndex: entry.threadIndex,
        occurrenceIndex,
        start,
        end,
      })
      occurrenceIndex += 1
      fromIndex = end
    }
  }

  return matches
}

export function nextConversationMatchIndex(
  currentIndex: number,
  total: number,
  direction: 1 | -1
): number {
  if (total <= 0) return -1
  const safeCurrent =
    currentIndex >= 0 && currentIndex < total ? currentIndex : 0
  return (safeCurrent + direction + total) % total
}
