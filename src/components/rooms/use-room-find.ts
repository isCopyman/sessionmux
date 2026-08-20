"use client"

import { useMemo } from "react"
import type { RefObject } from "react"

import type { ConversationFindEntry } from "@/lib/conversation-find"
import {
  useTranscriptFind,
  type TranscriptFindEntry,
  type TranscriptFindState,
} from "@/components/transcript/use-transcript-find"

/** Room post rows carry this marker; date separators and virtua chrome do not. */
export const ROOM_FIND_ROW_SELECTOR = "article[data-room-post-content]"

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
  viewportRef?: RefObject<HTMLElement | null>
  /**
   * Mount an off-screen virtualized row (display-order index, including date
   * separators) before the highlight pass.
   */
  scrollToIndex?: (
    index: number,
    opts?: { align?: "start" | "center" | "end" | "nearest" }
  ) => void
}

export type RoomFindState = TranscriptFindState

/**
 * Ctrl+F over a Room timeline, rewired onto `useTranscriptFind`.
 *
 * Maps Room's `{itemKey, threadIndex}` entries to the scaffold `{key, index}`
 * shape. The highlighter is pointed at `[data-room-post-content]` so date
 * separators (virtual rows) are not treated as hits.
 *
 * The `findPagedFromRef` anti-spin latch (same oldestId + still truncated →
 * stop) is Room-specific and lives in `rooms-page` wiring, not here.
 */
export function useRoomFind({
  entries,
  isActive,
  rootRef,
  viewportRef,
  scrollToIndex,
}: UseRoomFindArgs): RoomFindState {
  const mapped = useMemo<TranscriptFindEntry[]>(
    () =>
      entries.map((entry) => ({
        key: entry.itemKey,
        index: entry.threadIndex,
        text: entry.text,
      })),
    [entries]
  )

  return useTranscriptFind({
    entries: mapped,
    isActive,
    rootRef,
    viewportRef,
    scrollToIndex,
    rowSelector: ROOM_FIND_ROW_SELECTOR,
  })
}
