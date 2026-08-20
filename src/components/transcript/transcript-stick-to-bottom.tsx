"use client"

import { memo, useEffect, useRef } from "react"
import { useStickToBottomContext } from "use-stick-to-bottom"

export interface TranscriptStickToBottomApi {
  isAtBottom: boolean
  scrollToBottom: ReturnType<typeof useStickToBottomContext>["scrollToBottom"]
  scrollRef: ReturnType<typeof useStickToBottomContext>["scrollRef"]
  stopScroll: ReturnType<typeof useStickToBottomContext>["stopScroll"]
}

/**
 * Reads the surrounding `StickToBottom` tree. VirtualizedTranscript uses
 * `scrollRef` / `stopScroll`; AutoScrollOnSend uses `scrollToBottom`.
 */
export function useTranscriptStickToBottom(): TranscriptStickToBottomApi {
  const { isAtBottom, scrollToBottom, scrollRef, stopScroll } =
    useStickToBottomContext()
  return { isAtBottom, scrollToBottom, scrollRef, stopScroll }
}

/**
 * Session `AutoScrollOnSend` rewired onto the shared stick-to-bottom API.
 * Renders nothing; a rising `autoScrollSignal` jumps the viewport to the end.
 */
export const TranscriptStickToBottom = memo(function TranscriptStickToBottom({
  autoScrollSignal = 0,
}: {
  autoScrollSignal?: number
}) {
  const { scrollToBottom } = useTranscriptStickToBottom()
  const lastSignalRef = useRef(autoScrollSignal)

  useEffect(() => {
    if (autoScrollSignal === lastSignalRef.current) return
    lastSignalRef.current = autoScrollSignal

    scrollToBottom()
    const rafId = requestAnimationFrame(() => {
      scrollToBottom()
    })
    return () => {
      cancelAnimationFrame(rafId)
    }
  }, [scrollToBottom, autoScrollSignal])

  return null
})
