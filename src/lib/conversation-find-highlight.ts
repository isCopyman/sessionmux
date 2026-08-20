export const CONVERSATION_FIND_HIGHLIGHT = "codeg-conversation-find-match"
export const CONVERSATION_FIND_CURRENT_HIGHLIGHT =
  "codeg-conversation-find-current"
const CONVERSATION_FIND_STYLE_ID = "codeg-conversation-find-styles"

interface HighlightRegistryLike {
  set(name: string, highlight: unknown): void
  delete(name: string): void
}

type HighlightConstructorLike = new (...ranges: Range[]) => unknown

function highlightApi(): {
  registry: HighlightRegistryLike
  HighlightCtor: HighlightConstructorLike
} | null {
  const registry = (globalThis.CSS as unknown as { highlights?: unknown })
    ?.highlights as HighlightRegistryLike | undefined
  const HighlightCtor = (globalThis as unknown as { Highlight?: unknown })
    .Highlight as HighlightConstructorLike | undefined
  return registry && HighlightCtor ? { registry, HighlightCtor } : null
}

function ensureConversationFindStyles(): void {
  if (document.getElementById(CONVERSATION_FIND_STYLE_ID)) return
  const style = document.createElement("style")
  style.id = CONVERSATION_FIND_STYLE_ID
  // Kept out of globals.css because Turbopack currently rejects the standard
  // ::highlight pseudo-element even though WebView2 renders it correctly.
  style.textContent = `
::highlight(${CONVERSATION_FIND_HIGHLIGHT}) {
  color: inherit;
  background-color: oklch(0.82 0.16 85 / 0.42);
}
::highlight(${CONVERSATION_FIND_CURRENT_HIGHLIGHT}) {
  color: inherit;
  background-color: oklch(0.82 0.16 85 / 0.78);
}`
  document.head.append(style)
}

interface TextSegment {
  node: Text
  start: number
  end: number
}

/** Build DOM Ranges without rewriting React-owned text nodes. */
export function findTextRanges(root: Element, query: string): Range[] {
  if (!query) return []
  const segments: TextSegment[] = []
  let text = ""
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const parent = node.parentElement
      return parent?.closest("[data-conversation-search-exclude]")
        ? NodeFilter.FILTER_REJECT
        : NodeFilter.FILTER_ACCEPT
    },
  })
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const value = node.nodeValue ?? ""
    if (!value) continue
    const start = text.length
    text += value
    segments.push({ node: node as Text, start, end: text.length })
  }

  const needle = query.toLocaleLowerCase()
  const haystack = text.toLocaleLowerCase()
  const ranges: Range[] = []
  let fromIndex = 0

  while (fromIndex <= haystack.length) {
    const start = haystack.indexOf(needle, fromIndex)
    if (start < 0) break
    const end = start + query.length
    const startSegment = segments.find((segment) => start < segment.end)
    const endSegment = segments.find(
      (segment) => end > segment.start && end <= segment.end
    )
    if (startSegment && endSegment) {
      const range = document.createRange()
      range.setStart(startSegment.node, start - startSegment.start)
      range.setEnd(endSegment.node, end - endSegment.start)
      ranges.push(range)
    }
    fromIndex = end
  }

  return ranges
}

export function clearConversationFindHighlights(root?: Element | null): void {
  const api = highlightApi()
  api?.registry.delete(CONVERSATION_FIND_HIGHLIGHT)
  api?.registry.delete(CONVERSATION_FIND_CURRENT_HIGHLIGHT)
  root
    ?.querySelectorAll("[data-conversation-find-current]")
    .forEach((element) =>
      element.removeAttribute("data-conversation-find-current")
    )
}

// Two row markers, one walker: the Session transcript numbers its rows for the
// virtualizer, the Room timeline is a plain list and says so.
const FIND_ROW_SELECTOR = "[data-virtual-item-index], [data-find-row-index]"

/**
 * Highlight all currently mounted matches and emphasize the selected one.
 * Off-screen rows are virtualized; a MutationObserver in MessageListView calls
 * this again as navigation mounts the target row.
 */
export function applyConversationFindHighlights(
  root: Element,
  query: string,
  current: { threadIndex: number; occurrenceIndex: number } | null
): Range | null {
  clearConversationFindHighlights(root)
  if (!query) return null

  const allRanges: Range[] = []
  let currentRange: Range | null = null
  const rows = root.querySelectorAll<HTMLElement>(FIND_ROW_SELECTOR)
  for (const row of rows) {
    const threadIndex = Number(
      row.dataset.virtualItemIndex ?? row.dataset.findRowIndex
    )
    const content = row.querySelector("[data-conversation-search-content]")
    if (!content) continue
    const ranges = findTextRanges(content, query)
    allRanges.push(...ranges)
    if (current?.threadIndex === threadIndex) {
      currentRange = ranges[current.occurrenceIndex] ?? null
      row.setAttribute("data-conversation-find-current", "")
    }
  }

  const api = highlightApi()
  if (api) {
    ensureConversationFindStyles()
    if (allRanges.length > 0) {
      api.registry.set(
        CONVERSATION_FIND_HIGHLIGHT,
        new api.HighlightCtor(...allRanges)
      )
    }
    if (currentRange) {
      api.registry.set(
        CONVERSATION_FIND_CURRENT_HIGHLIGHT,
        new api.HighlightCtor(currentRange)
      )
    }
  }

  return currentRange
}

function getRangeRect(range: Range): DOMRect | null {
  const rects = Array.from(range.getClientRects())
  const visibleRect = rects.find((rect) => rect.width > 0 || rect.height > 0)
  return visibleRect ?? range.getBoundingClientRect()
}

/**
 * Keep a visible match steady; center it only after it leaves the viewport.
 *
 * `viewportElement` is for hosts whose scroller carries no class of its own —
 * the Room timeline's OverlayScrollbars viewport is only reachable through the
 * `ScrollArea` `onViewportRef` callback.
 */
export function revealConversationFindRange(
  root: Element,
  range: Range,
  viewportElement?: HTMLElement | null
): boolean {
  const viewport =
    viewportElement ?? root.querySelector<HTMLElement>(".scrollbar-thin")
  if (!viewport) return false

  const rangeRect = getRangeRect(range)
  const viewportRect = viewport.getBoundingClientRect()
  if (
    !rangeRect ||
    !Number.isFinite(rangeRect.top) ||
    !Number.isFinite(rangeRect.height) ||
    !Number.isFinite(viewportRect.top) ||
    !Number.isFinite(viewportRect.height)
  ) {
    return false
  }

  const rangeBottom = rangeRect.top + rangeRect.height
  const viewportBottom = viewportRect.top + viewportRect.height
  if (viewportRect.top <= rangeRect.top && rangeBottom <= viewportBottom) {
    return true
  }

  const matchCenter = rangeRect.top + rangeRect.height / 2
  const viewportCenter = viewportRect.top + viewportRect.height / 2
  const delta =
    rangeRect.height > viewportRect.height
      ? rangeRect.top - viewportRect.top
      : matchCenter - viewportCenter
  if (Math.abs(delta) > 1) {
    viewport.scrollTo({ top: viewport.scrollTop + delta, behavior: "auto" })
  }
  return true
}
