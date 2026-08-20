"use client"

import { useEffect } from "react"

export interface UseTranscriptInfiniteScrollArgs {
  hasOlder: boolean
  isLoading: boolean
  onLoadOlder: () => void
  isActive?: boolean
  /**
   * Find↔pagination coupling: while a non-empty query is active, keep paging
   * until `hasOlder` is false so the match count covers the full transcript
   * rather than the initial tail window.
   */
  findOpen?: boolean
  findQuery?: string
}

export interface UseTranscriptInfiniteScrollResult {
  hasOlder: boolean
  isLoading: boolean
  onLoadOlder: () => void
  searchingOlderHistory: boolean
}

/**
 * Host-side reverse-infinite-scroll wiring, including the find-driven
 * "page until the query covers the whole session" loop.
 *
 * Near-top crossing and prepend-shift live in VirtualizedTranscript (they
 * are virtua-specific). This hook is the data/find side of that pair.
 */
export function useTranscriptInfiniteScroll({
  hasOlder,
  isLoading,
  onLoadOlder,
  isActive = true,
  findOpen = false,
  findQuery = "",
}: UseTranscriptInfiniteScrollArgs): UseTranscriptInfiniteScrollResult {
  const searchingOlderHistory =
    findOpen && findQuery.length > 0 && (hasOlder || isLoading)

  useEffect(() => {
    if (
      !isActive ||
      !findOpen ||
      findQuery.length === 0 ||
      !hasOlder ||
      isLoading
    ) {
      return
    }
    onLoadOlder()
  }, [findOpen, findQuery, hasOlder, isActive, isLoading, onLoadOlder])

  return {
    hasOlder,
    isLoading,
    onLoadOlder,
    searchingOlderHistory,
  }
}
