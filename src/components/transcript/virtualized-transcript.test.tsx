import { createRef, type ReactNode } from "react"
import { render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { TranscriptScrollApi } from "./virtualized-transcript"

const testState = vi.hoisted(() => ({
  scrollRef: { current: null as HTMLDivElement | null },
  stopScroll: vi.fn(),
  scrollTo: vi.fn(),
  scrollToIndex: vi.fn(),
  captureCache: vi.fn(),
}))

vi.mock("use-stick-to-bottom", () => ({
  useStickToBottomContext: () => ({
    scrollRef: testState.scrollRef,
    stopScroll: testState.stopScroll,
    isAtBottom: true,
    scrollToBottom: vi.fn(),
  }),
}))

vi.mock("virtua", async () => {
  const React = await import("react")
  const MockVirtualizer = React.forwardRef(
    (
      {
        children,
        cache,
      }: {
        children: ReactNode
        cache?: unknown
      },
      ref
    ) => {
      testState.captureCache(cache)
      React.useImperativeHandle(ref, () => ({
        cache: { measured: true },
        scrollOffset: 720,
        scrollSize: 1600,
        viewportSize: 600,
        findItemIndex: () => 0,
        getItemOffset: () => 0,
        getItemSize: () => 0,
        scrollToIndex: testState.scrollToIndex,
        scrollTo: testState.scrollTo,
        scrollBy: vi.fn(),
      }))
      return <>{children}</>
    }
  )
  MockVirtualizer.displayName = "MockVirtualizer"
  return {
    Virtualizer: MockVirtualizer,
  }
})

vi.mock("@/components/ai-elements/message-thread", () => ({
  MessageThreadContent: ({
    children,
    scrollClassName,
  }: {
    children: ReactNode
    scrollClassName?: string
  }) => (
    <div
      ref={(element) => {
        testState.scrollRef.current = element
      }}
      className={scrollClassName}
      data-testid="viewport"
    >
      {children}
    </div>
  ),
}))

import { VirtualizedTranscript } from "./virtualized-transcript"

beforeEach(() => {
  testState.scrollRef.current = null
  testState.stopScroll.mockClear()
  testState.scrollTo.mockClear()
  testState.scrollToIndex.mockClear()
  testState.captureCache.mockClear()
})

describe("VirtualizedTranscript", () => {
  it("stamps data-virtual-item-index in display order for generic items", () => {
    render(
      <VirtualizedTranscript
        items={[
          { id: "a", label: "alpha" },
          { id: "b", label: "beta" },
          { id: "c", label: "gamma" },
        ]}
        getItemKey={(item) => item.id}
        renderItem={(item) => <span>{item.label}</span>}
      />
    )

    expect(
      screen.getByText("alpha").closest("[data-virtual-item-index]")
    ).toHaveAttribute("data-virtual-item-index", "0")
    expect(
      screen.getByText("beta").closest("[data-virtual-item-index]")
    ).toHaveAttribute("data-virtual-item-index", "1")
    expect(
      screen.getByText("gamma").closest("[data-virtual-item-index]")
    ).toHaveAttribute("data-virtual-item-index", "2")
  })

  it("uses getItemKey for row identity, not the array index", () => {
    const { rerender } = render(
      <VirtualizedTranscript
        items={[{ id: "keep" }, { id: "tail" }]}
        getItemKey={(item) => item.id}
        renderItem={(item) => <span>{item.id}</span>}
      />
    )
    const keep = screen.getByText("keep").closest("[data-virtual-item-index]")
    rerender(
      <VirtualizedTranscript
        items={[{ id: "prepended" }, { id: "keep" }, { id: "tail" }]}
        getItemKey={(item) => item.id}
        renderItem={(item) => <span>{item.id}</span>}
      />
    )
    expect(screen.getByText("keep").closest("[data-virtual-item-index]")).toBe(
      keep
    )
    expect(
      screen.getByText("keep").closest("[data-virtual-item-index]")
    ).toHaveAttribute("data-virtual-item-index", "1")
  })

  it("renders emptyState when items is empty", () => {
    render(
      <VirtualizedTranscript
        items={[]}
        getItemKey={() => "x"}
        renderItem={() => null}
        emptyState={<p>nothing here</p>}
      />
    )
    expect(screen.getByText("nothing here")).toBeInTheDocument()
  })

  it("does not stamp data-virtual-item-index on the load-older row", () => {
    render(
      <VirtualizedTranscript
        items={[{ id: "only" }]}
        getItemKey={(item) => item.id}
        renderItem={(item) => <span>{item.id}</span>}
        hasOlder
        loadOlderLabel="Load earlier"
      />
    )
    expect(screen.getByRole("button", { name: "Load earlier" })).toBeVisible()
    expect(
      screen
        .getByRole("button", { name: "Load earlier" })
        .closest("[data-virtual-item-index]")
    ).toBeNull()
    expect(
      screen.getByText("only").closest("[data-virtual-item-index]")
    ).toHaveAttribute("data-virtual-item-index", "0")
  })

  it("offsets scrollToIndex by the loader row when hasOlder is set", () => {
    const scrollApiRef = createRef<TranscriptScrollApi | null>()
    render(
      <VirtualizedTranscript
        items={[{ id: "a" }, { id: "b" }]}
        getItemKey={(item) => item.id}
        renderItem={(item) => <span>{item.id}</span>}
        hasOlder
        scrollApiRef={scrollApiRef}
      />
    )
    scrollApiRef.current?.scrollToIndex(1, { align: "center" })
    expect(testState.scrollToIndex).toHaveBeenCalledWith(2, { align: "center" })
  })
})
