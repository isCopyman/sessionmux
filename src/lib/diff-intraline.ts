export type IntralineKind = "equal" | "removed" | "added"

export interface IntralineSpan {
  kind: IntralineKind
  text: string
}

const MIN_SIMILARITY = 0.3
const MAX_LINE_CHARS = 2000
const MAX_BLOCK_LINES = 80
const MAX_TOKEN_CELLS = 40_000

const WORD_CHAR = /[A-Za-z0-9_]/

export function tokenizeForIntraline(text: string): string[] {
  const tokens: string[] = []
  let index = 0
  while (index < text.length) {
    const char = text[index] ?? ""
    if (/\s/.test(char)) {
      let end = index + 1
      while (end < text.length && /\s/.test(text[end] ?? "")) end += 1
      tokens.push(text.slice(index, end))
      index = end
      continue
    }
    if (WORD_CHAR.test(char)) {
      let end = index + 1
      while (end < text.length && WORD_CHAR.test(text[end] ?? "")) end += 1
      tokens.push(text.slice(index, end))
      index = end
      continue
    }
    tokens.push(char)
    index += 1
  }
  return tokens
}

function lcsLength(left: string[], right: string[]): number {
  if (left.length === 0 || right.length === 0) return 0
  if (left.length * right.length > MAX_TOKEN_CELLS) return 0
  const previous = new Array<number>(right.length + 1).fill(0)
  const current = new Array<number>(right.length + 1).fill(0)
  for (let i = 1; i <= left.length; i++) {
    current[0] = 0
    for (let j = 1; j <= right.length; j++) {
      current[j] =
        left[i - 1] === right[j - 1]
          ? (previous[j - 1] ?? 0) + 1
          : Math.max(previous[j] ?? 0, current[j - 1] ?? 0)
    }
    for (let j = 0; j <= right.length; j++) {
      previous[j] = current[j] ?? 0
    }
  }
  return previous[right.length] ?? 0
}

export function similarity(left: string, right: string): number {
  if (left === right) return 1
  if (!left || !right) return 0
  const leftTokens = tokenizeForIntraline(left)
  const rightTokens = tokenizeForIntraline(right)
  const total = leftTokens.length + rightTokens.length
  if (total === 0) return 1
  return (2 * lcsLength(leftTokens, rightTokens)) / total
}

function mergeSpans(spans: IntralineSpan[]): IntralineSpan[] {
  const merged: IntralineSpan[] = []
  for (const span of spans) {
    const last = merged[merged.length - 1]
    if (last && last.kind === span.kind) {
      last.text += span.text
    } else {
      merged.push({ ...span })
    }
  }
  return merged.filter((span) => span.text.length > 0)
}

export function diffTokens(
  oldText: string,
  newText: string
): { oldSpans: IntralineSpan[]; newSpans: IntralineSpan[] } | null {
  if (oldText === newText) return null
  if (oldText.length > MAX_LINE_CHARS || newText.length > MAX_LINE_CHARS) {
    return null
  }
  const oldTokens = tokenizeForIntraline(oldText)
  const newTokens = tokenizeForIntraline(newText)
  if (oldTokens.length * newTokens.length > MAX_TOKEN_CELLS) return null

  let prefix = 0
  while (
    prefix < oldTokens.length &&
    prefix < newTokens.length &&
    oldTokens[prefix] === newTokens[prefix]
  ) {
    prefix += 1
  }
  let suffix = 0
  while (
    suffix < oldTokens.length - prefix &&
    suffix < newTokens.length - prefix &&
    oldTokens[oldTokens.length - 1 - suffix] ===
      newTokens[newTokens.length - 1 - suffix]
  ) {
    suffix += 1
  }

  const oldMid = oldTokens.slice(prefix, oldTokens.length - suffix)
  const newMid = newTokens.slice(prefix, newTokens.length - suffix)
  const prefixText = oldTokens.slice(0, prefix).join("")
  const suffixText = oldTokens.slice(oldTokens.length - suffix).join("")

  const oldSpans: IntralineSpan[] = []
  const newSpans: IntralineSpan[] = []
  if (prefixText) {
    oldSpans.push({ kind: "equal", text: prefixText })
    newSpans.push({ kind: "equal", text: prefixText })
  }

  if (oldMid.length === 0 && newMid.length === 0) {
    return null
  }

  if (oldMid.length * newMid.length > MAX_TOKEN_CELLS) return null

  const rows = oldMid.length
  const cols = newMid.length
  const table: number[][] = Array.from({ length: rows + 1 }, () =>
    new Array<number>(cols + 1).fill(0)
  )
  for (let i = 1; i <= rows; i++) {
    const row = table[i]
    const prev = table[i - 1]
    if (!row || !prev) continue
    for (let j = 1; j <= cols; j++) {
      row[j] =
        oldMid[i - 1] === newMid[j - 1]
          ? (prev[j - 1] ?? 0) + 1
          : Math.max(prev[j] ?? 0, row[j - 1] ?? 0)
    }
  }

  const oldMidSpans: IntralineSpan[] = []
  const newMidSpans: IntralineSpan[] = []
  let i = rows
  let j = cols
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldMid[i - 1] === newMid[j - 1]) {
      const token = oldMid[i - 1] ?? ""
      oldMidSpans.push({ kind: "equal", text: token })
      newMidSpans.push({ kind: "equal", text: token })
      i -= 1
      j -= 1
    } else if (
      j > 0 &&
      (i === 0 || (table[i]?.[j - 1] ?? 0) >= (table[i - 1]?.[j] ?? 0))
    ) {
      newMidSpans.push({ kind: "added", text: newMid[j - 1] ?? "" })
      j -= 1
    } else {
      oldMidSpans.push({ kind: "removed", text: oldMid[i - 1] ?? "" })
      i -= 1
    }
  }
  oldMidSpans.reverse()
  newMidSpans.reverse()
  oldSpans.push(...oldMidSpans)
  newSpans.push(...newMidSpans)

  if (suffixText) {
    oldSpans.push({ kind: "equal", text: suffixText })
    newSpans.push({ kind: "equal", text: suffixText })
  }

  const mergedOld = mergeSpans(oldSpans)
  const mergedNew = mergeSpans(newSpans)
  const oldHasEqual = mergedOld.some((span) => span.kind === "equal")
  const newHasEqual = mergedNew.some((span) => span.kind === "equal")
  const oldHasChange = mergedOld.some((span) => span.kind === "removed")
  const newHasChange = mergedNew.some((span) => span.kind === "added")
  if (!oldHasEqual || !newHasEqual || !oldHasChange || !newHasChange) {
    return null
  }
  return { oldSpans: mergedOld, newSpans: mergedNew }
}

export function pairDeleteAddLines(
  deleted: string[],
  added: string[]
): Array<{ deletedIndex: number; addedIndex: number }> {
  const pairs: Array<{ deletedIndex: number; addedIndex: number }> = []
  if (deleted.length === 0 || added.length === 0) return pairs
  if (deleted.length > MAX_BLOCK_LINES || added.length > MAX_BLOCK_LINES) {
    return pairs
  }

  if (deleted.length === added.length) {
    deleted.forEach((text, index) => {
      const other = added[index] ?? ""
      if (similarity(text, other) >= MIN_SIMILARITY) {
        pairs.push({ deletedIndex: index, addedIndex: index })
      }
    })
    return pairs
  }

  const usedAdds = new Set<number>()
  let minAdd = 0
  deleted.forEach((delText, deletedIndex) => {
    let best = -1
    let bestScore = MIN_SIMILARITY
    for (let addedIndex = minAdd; addedIndex < added.length; addedIndex++) {
      if (usedAdds.has(addedIndex)) continue
      const score = similarity(delText, added[addedIndex] ?? "")
      if (score > bestScore) {
        bestScore = score
        best = addedIndex
      }
    }
    if (best >= 0) {
      usedAdds.add(best)
      minAdd = best + 1
      pairs.push({ deletedIndex, addedIndex: best })
    }
  })
  return pairs
}

export function decorateDeleteAddBlock(
  deletedTexts: string[],
  addedTexts: string[]
): {
  deletedSpans: Array<IntralineSpan[] | null>
  addedSpans: Array<IntralineSpan[] | null>
} {
  const deletedSpans = deletedTexts.map(() => null)
  const addedSpans = addedTexts.map(() => null)
  for (const pair of pairDeleteAddLines(deletedTexts, addedTexts)) {
    const oldText = deletedTexts[pair.deletedIndex] ?? ""
    const newText = addedTexts[pair.addedIndex] ?? ""
    const diff = diffTokens(oldText, newText)
    if (!diff) continue
    deletedSpans[pair.deletedIndex] = diff.oldSpans
    addedSpans[pair.addedIndex] = diff.newSpans
  }
  return { deletedSpans, addedSpans }
}
