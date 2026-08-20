"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import type { RefObject } from "react"

import {
  findConversationMatches,
  nextConversationMatchIndex,
  type ConversationFindEntry,
} from "@/lib/conversation-find"
import {
  applyConversationFindHighlights,
  clearConversationFindHighlights,
  revealConversationFindRange,
} from "@/lib/conversation-find-highlight"

export interface TranscriptFindEntry {
  key: string
  index: number
  text: string
}

export interface UseTranscriptFindArgs {
  entries: readonly TranscriptFindEntry[]
  /**
   * Only the focused surface arms Ctrl/Cmd+F. Session gates this with
   * `isActive && showMessageNav`; Room passes `isActive` alone.
   */
  isActive: boolean
  /** Element the highlighter walks; also the find bar's positioning context. */
  rootRef: RefObject<HTMLElement | null>
  /**
   * Scroller to move when the selected match is off-screen. Spec shape is a
   * value; Room's OverlayScrollbars viewport is assigned later via a ref.
   */
  viewportElement?: HTMLElement | null
  viewportRef?: RefObject<HTMLElement | null>
  /**
   * Virtualized hosts: mount an off-screen row (by display-order index)
   * before the highlight pass. Not in the dispatch spec's argument list —
   * required to keep Session's off-screen locate behavior. See report.
   */
  scrollToIndex?: (
    index: number,
    opts?: { align?: "start" | "center" | "end" | "nearest" }
  ) => void
  /**
   * Row selector for the highlighter. Default is FIND_ROW_SELECTOR
   * (`[data-virtual-item-index], [data-find-row-index]`). Room passes a
   * selector that includes `[data-room-post-content]` so date-separator
   * virtual rows and the virtua wrapper are not painted as hits.
   */
  rowSelector?: string
}

export interface TranscriptFindState {
  query: string
  /** 1-based position of the selected match, 0 when there is none. */
  current: number
  total: number
  open: boolean
  onNext: () => void
  onPrevious: () => void
  onClose: () => void
  onQueryChange: (query: string) => void
  focusToken: number
}

/**
 * Ctrl/Cmd+F over a transcript-like list. Match list from `conversation-find`;
 * paint from `conversation-find-highlight`. Off-screen virtualized rows are
 * mounted via `scrollToIndex` then highlighted once they exist in the DOM.
 */
export function useTranscriptFind({
  entries,
  isActive,
  rootRef,
  viewportElement,
  viewportRef,
  scrollToIndex,
  rowSelector,
}: UseTranscriptFindArgs): TranscriptFindState {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState("")
  const [focusToken, setFocusToken] = useState(0)
  const [activeMatchId, setActiveMatchId] = useState<string | null>(null)
  const pendingFindScrollMatchIdRef = useRef<string | null>(null)

  const conversationEntries = useMemo<ConversationFindEntry[]>(
    () =>
      entries.map((entry) => ({
        itemKey: entry.key,
        threadIndex: entry.index,
        text: entry.text,
      })),
    [entries]
  )

  const matches = useMemo(
    () => (open ? findConversationMatches(conversationEntries, query) : []),
    [conversationEntries, open, query]
  )
  const activeIndex = activeMatchId
    ? matches.findIndex((match) => match.id === activeMatchId)
    : -1
  const currentIndex =
    activeIndex >= 0 ? activeIndex : matches.length > 0 ? 0 : -1
  const activeMatch = currentIndex >= 0 ? matches[currentIndex] : null
  const selectedMatchId = activeMatch?.id ?? null
  const selectedThreadIndex = activeMatch?.threadIndex ?? null

  const openFind = useCallback(() => {
    setOpen(true)
    setFocusToken((token) => token + 1)
  }, [])

  const onClose = useCallback(() => {
    pendingFindScrollMatchIdRef.current = null
    setOpen(false)
    setQuery("")
    setActiveMatchId(null)
  }, [])

  const onQueryChange = useCallback((next: string) => {
    pendingFindScrollMatchIdRef.current = null
    setQuery(next)
    setActiveMatchId(null)
  }, [])

  const move = useCallback(
    (direction: 1 | -1) => {
      const nextIndex = nextConversationMatchIndex(
        currentIndex,
        matches.length,
        direction
      )
      if (nextIndex >= 0) setActiveMatchId(matches[nextIndex].id)
    },
    [currentIndex, matches]
  )
  const onNext = useCallback(() => move(1), [move])
  const onPrevious = useCallback(() => move(-1), [move])

  useEffect(() => {
    if (!isActive) return
    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        event.altKey ||
        (!event.ctrlKey && !event.metaKey) ||
        event.key.toLocaleLowerCase() !== "f"
      ) {
        return
      }
      const target = event.target
      if (
        target instanceof Element &&
        target.closest(".monaco-editor, iframe, [data-native-find-scope]")
      ) {
        return
      }
      event.preventDefault()
      event.stopPropagation()
      openFind()
    }
    window.addEventListener("keydown", handleKeyDown, true)
    return () => window.removeEventListener("keydown", handleKeyDown, true)
  }, [isActive, openFind])

  useEffect(() => {
    if (!open || selectedMatchId === null || selectedThreadIndex === null) {
      pendingFindScrollMatchIdRef.current = null
      return
    }
    pendingFindScrollMatchIdRef.current = selectedMatchId
    const root = rootRef.current
    const targetRow = root?.querySelector(
      `[data-virtual-item-index="${selectedThreadIndex}"], [data-find-row-index="${selectedThreadIndex}"]`
    )
    // Off-screen virtualized rows do not exist in the DOM yet. Mount first;
    // the highlight pass below then performs the precise text-level centering.
    if (!targetRow) {
      scrollToIndex?.(selectedThreadIndex, { align: "center" })
    }
  }, [open, rootRef, scrollToIndex, selectedMatchId, selectedThreadIndex])

  useEffect(() => {
    const root = rootRef.current
    if (!root || !isActive || !open || query.length === 0) {
      pendingFindScrollMatchIdRef.current = null
      clearConversationFindHighlights(root)
      return
    }
    const viewport = viewportElement ?? viewportRef?.current ?? null
    let rafId = 0
    const paint = () => {
      rafId = 0
      const currentRange = applyConversationFindHighlights(
        root,
        query,
        activeMatch,
        rowSelector
      )
      if (
        currentRange &&
        activeMatch &&
        pendingFindScrollMatchIdRef.current === activeMatch.id &&
        revealConversationFindRange(root, currentRange, viewport)
      ) {
        pendingFindScrollMatchIdRef.current = null
      }
    }
    const schedulePaint = () => {
      if (rafId) cancelAnimationFrame(rafId)
      rafId = requestAnimationFrame(paint)
    }
    schedulePaint()
    const observer = new MutationObserver(schedulePaint)
    observer.observe(root, { childList: true, subtree: true })
    const scrollViewport =
      viewport ?? root.querySelector<HTMLElement>(".scrollbar-thin")
    scrollViewport?.addEventListener("scroll", schedulePaint, { passive: true })
    return () => {
      if (rafId) cancelAnimationFrame(rafId)
      observer.disconnect()
      scrollViewport?.removeEventListener("scroll", schedulePaint)
      clearConversationFindHighlights(root)
    }
  }, [
    activeMatch,
    entries,
    isActive,
    open,
    query,
    rootRef,
    viewportElement,
    viewportRef,
    rowSelector,
  ])

  return {
    query,
    current: activeMatch ? currentIndex + 1 : 0,
    total: matches.length,
    open,
    onNext,
    onPrevious,
    onClose,
    onQueryChange,
    focusToken,
  }
}
