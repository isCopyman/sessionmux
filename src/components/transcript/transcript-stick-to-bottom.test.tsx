import { render, renderHook } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

const testState = vi.hoisted(() => ({
  isAtBottom: true,
  scrollToBottom: vi.fn(),
  scrollRef: { current: null as HTMLElement | null },
  stopScroll: vi.fn(),
}))

vi.mock("use-stick-to-bottom", () => ({
  useStickToBottomContext: () => ({
    isAtBottom: testState.isAtBottom,
    scrollToBottom: testState.scrollToBottom,
    scrollRef: testState.scrollRef,
    stopScroll: testState.stopScroll,
  }),
}))

import {
  TranscriptStickToBottom,
  useTranscriptStickToBottom,
} from "./transcript-stick-to-bottom"

beforeEach(() => {
  testState.isAtBottom = true
  testState.scrollToBottom.mockClear()
  testState.stopScroll.mockClear()
})

describe("useTranscriptStickToBottom", () => {
  it("exposes isAtBottom, scrollToBottom, scrollRef, and stopScroll", () => {
    const { result } = renderHook(() => useTranscriptStickToBottom())
    expect(result.current.isAtBottom).toBe(true)
    expect(result.current.scrollToBottom).toBe(testState.scrollToBottom)
    expect(result.current.scrollRef).toBe(testState.scrollRef)
    expect(result.current.stopScroll).toBe(testState.stopScroll)
  })
})

describe("TranscriptStickToBottom", () => {
  it("does not scroll on the initial signal", () => {
    render(<TranscriptStickToBottom autoScrollSignal={0} />)
    expect(testState.scrollToBottom).not.toHaveBeenCalled()
  })

  it("jumps to bottom when autoScrollSignal advances", () => {
    const view = render(<TranscriptStickToBottom autoScrollSignal={0} />)
    view.rerender(<TranscriptStickToBottom autoScrollSignal={1} />)
    expect(testState.scrollToBottom).toHaveBeenCalled()
  })
})
