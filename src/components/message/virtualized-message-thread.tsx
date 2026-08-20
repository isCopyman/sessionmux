"use client"

/**
 * Session-specialized virtualizer: a thin shell over the generic
 * `VirtualizedTranscript`. Kept at this path so existing imports and the
 * 242-line test file do not move.
 */
export {
  shouldShiftForPrepend,
  VirtualizedTranscript as VirtualizedMessageThread,
  type VirtualizedThreadViewState,
} from "@/components/transcript/virtualized-transcript"
