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

interface UseRoomFindArgs {
  entries: readonly ConversationFindEntry[]
  /**
   * Only the focused tab arms the Ctrl+F listener. Session panels gate the
   * identical window listener the same way (`MessageListView`), and at most one
   * tab is active, so the two never both claim the keystroke.
   */
  isActive: boolean
  /** Element the highlighter walks; also the find bar's positioning context. */
  rootRef: RefObject<HTMLElement | null>
  /** Scroller to move when the selected match is off-screen. */
  viewportRef: RefObject<HTMLElement | null>
}

export interface RoomFindState {
  open: boolean
  query: string
  /** 1-based position of the selected match, 0 when there is none. */
  current: number
  total: number
  focusToken: number
  onQueryChange: (query: string) => void
  onNext: () => void
  onPrevious: () => void
  onClose: () => void
}

/**
 * Ctrl+F over a Room timeline, on the same two pieces the Session transcript
 * uses: `conversation-find` for the literal match list and
 * `conversation-find-highlight` for the CSS Custom Highlight ranges.
 *
 * The highlighter walks text nodes, so Markdown-rendered post bodies need no
 * special handling — but the match *count* comes from a plain-text projection
 * of the same posts (`roomMessagePlainText`), so the two can disagree on
 * queries that target Markdown syntax. See that function for the tradeoff.
 */
export function useRoomFind({
  entries,
  isActive,
  rootRef,
  viewportRef,
}: UseRoomFindArgs): RoomFindState {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState("")
  const [focusToken, setFocusToken] = useState(0)
  const [activeMatchId, setActiveMatchId] = useState<string | null>(null)

  const matches = useMemo(
    () => (open ? findConversationMatches(entries, query) : []),
    [entries, open, query]
  )
  const activeIndex = activeMatchId
    ? matches.findIndex((match) => match.id === activeMatchId)
    : -1
  const currentIndex =
    activeIndex >= 0 ? activeIndex : matches.length > 0 ? 0 : -1
  const activeMatch = currentIndex >= 0 ? matches[currentIndex] : null

  const openFind = useCallback(() => {
    setOpen(true)
    setFocusToken((token) => token + 1)
  }, [])

  const onClose = useCallback(() => {
    setOpen(false)
    setQuery("")
    setActiveMatchId(null)
  }, [])

  const onQueryChange = useCallback((next: string) => {
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
      const target = event.target as HTMLElement | null
      // Same carve-out as the Session panel: an embedded editor or frame keeps
      // its own find. The Room composer is deliberately not excluded.
      if (target?.closest(".monaco-editor, iframe, [data-native-find-scope]")) {
        return
      }
      event.preventDefault()
      event.stopPropagation()
      openFind()
    }
    window.addEventListener("keydown", handleKeyDown, true)
    return () => window.removeEventListener("keydown", handleKeyDown, true)
  }, [isActive, openFind])

  // Scroll only when the selection actually moved, so a repaint triggered by a
  // new post or a manual scroll never yanks the viewport back.
  const revealedRef = useRef<string | null>(null)
  useEffect(() => {
    const root = rootRef.current
    if (!root || !isActive || !open || query.length === 0) {
      revealedRef.current = null
      clearConversationFindHighlights(root)
      return
    }
    let rafId = 0
    const paint = () => {
      rafId = 0
      const range = applyConversationFindHighlights(root, query, activeMatch)
      if (
        range &&
        activeMatch &&
        revealedRef.current !== activeMatch.id &&
        revealConversationFindRange(root, range, viewportRef.current)
      ) {
        revealedRef.current = activeMatch.id
      }
    }
    const schedulePaint = () => {
      if (rafId) cancelAnimationFrame(rafId)
      rafId = requestAnimationFrame(paint)
    }
    schedulePaint()
    // Posts arrive mid-search (a woken Session replies); repaint so the new
    // body is highlighted too.
    const observer = new MutationObserver(schedulePaint)
    observer.observe(root, { childList: true, subtree: true })
    const viewport = viewportRef.current
    viewport?.addEventListener("scroll", schedulePaint, { passive: true })
    return () => {
      if (rafId) cancelAnimationFrame(rafId)
      observer.disconnect()
      viewport?.removeEventListener("scroll", schedulePaint)
      clearConversationFindHighlights(root)
    }
  }, [activeMatch, isActive, open, query, rootRef, viewportRef])

  return {
    open,
    query,
    current: activeMatch ? currentIndex + 1 : 0,
    total: matches.length,
    focusToken,
    onQueryChange,
    onNext,
    onPrevious,
    onClose,
  }
}
