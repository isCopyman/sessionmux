import type { ReactNode } from "react"
import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

const testState = vi.hoisted(() => ({
  scrollRef: { current: null as HTMLDivElement | null },
  stopScroll: vi.fn(),
  scrollTo: vi.fn(),
  captureCache: vi.fn(),
}))

vi.mock("use-stick-to-bottom", () => ({
  useStickToBottomContext: () => ({
    scrollRef: testState.scrollRef,
    stopScroll: testState.stopScroll,
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
        scrollToIndex: vi.fn(),
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

import { VirtualizedMessageThread } from "@/components/message/virtualized-message-thread"

function renderThread(
  content: ReactNode = <div data-testid="content">text</div>
) {
  return render(
    <VirtualizedMessageThread
      items={[{ id: "message-1" }]}
      getItemKey={(item) => item.id}
      renderItem={() => content}
    />
  )
}

function pointerDown(element: HTMLElement, button: number) {
  fireEvent(element, new MouseEvent("pointerdown", { bubbles: true, button }))
}

function keyDown(element: HTMLElement, key: string) {
  fireEvent.keyDown(element, { key })
}

beforeEach(() => {
  testState.scrollRef.current = null
  testState.stopScroll.mockClear()
  testState.scrollTo.mockClear()
  testState.captureCache.mockClear()
})

describe("VirtualizedMessageThread focus origin", () => {
  it("marks pointer-origin focus and clears it on blur", () => {
    renderThread()
    const viewport = screen.getByTestId("viewport")

    pointerDown(screen.getByTestId("content"), 0)

    expect(document.activeElement).toBe(viewport)
    expect(viewport).toHaveAttribute("data-focus-origin", "pointer")
    expect(viewport.className).toContain(
      "data-[focus-origin=pointer]:focus-visible:ring-0"
    )

    fireEvent.blur(viewport)
    expect(viewport).not.toHaveAttribute("data-focus-origin")
  })

  it("clears the pointer marker on keyboard input so the ring returns", () => {
    renderThread()
    const viewport = screen.getByTestId("viewport")

    pointerDown(screen.getByTestId("content"), 0)
    expect(viewport).toHaveAttribute("data-focus-origin", "pointer")

    // Switching to keyboard scrolling drops the marker, so the suppressing
    // `data-[focus-origin=pointer]` selector no longer matches and the
    // keyboard focus ring becomes visible again.
    keyDown(viewport, "ArrowDown")
    expect(viewport).not.toHaveAttribute("data-focus-origin")
    expect(document.activeElement).toBe(viewport)
  })

  it("keeps keyboard-origin focus distinguishable", () => {
    renderThread()
    const viewport = screen.getByTestId("viewport")

    viewport.focus()

    expect(document.activeElement).toBe(viewport)
    expect(viewport).not.toHaveAttribute("data-focus-origin")
    expect(viewport.className).toContain("focus-visible:ring-2")
  })

  it("does not mark focus when an interactive control is clicked", () => {
    renderThread(<button data-testid="action">Action</button>)
    const viewport = screen.getByTestId("viewport")

    pointerDown(screen.getByTestId("action"), 0)

    expect(viewport).not.toHaveAttribute("data-focus-origin")
    expect(document.activeElement).not.toBe(viewport)
  })

  it("does not mark focus for a right click", () => {
    renderThread()
    const viewport = screen.getByTestId("viewport")

    pointerDown(screen.getByTestId("content"), 2)

    expect(viewport).not.toHaveAttribute("data-focus-origin")
    expect(document.activeElement).not.toBe(viewport)
  })

  it("restores a warm non-bottom reading position before paint", () => {
    const virtualizerCache = { measured: "cache" } as never
    render(
      <VirtualizedMessageThread
        items={[{ id: "message-1" }]}
        getItemKey={(item) => item.id}
        renderItem={() => <div>message</div>}
        initialViewState={{
          scrollOffset: 420,
          atBottom: false,
          virtualItemCount: 1,
          virtualizerCache,
        }}
      />
    )

    expect(testState.captureCache).toHaveBeenCalledWith(virtualizerCache)
    expect(testState.stopScroll).toHaveBeenCalled()
    expect(testState.scrollTo).toHaveBeenCalledWith(420)
  })

  it("restores a saved offset after a cold remount even if the item count differs", () => {
    const view = render(
      <VirtualizedMessageThread
        items={[{ id: "message-1" }, { id: "message-2" }]}
        getItemKey={(item) => item.id}
        renderItem={() => <div>message</div>}
        initialViewState={{
          scrollOffset: 1800,
          atBottom: false,
          virtualItemCount: 21,
          virtualizerCache: null,
        }}
      />
    )

    expect(testState.stopScroll).toHaveBeenCalled()
    expect(testState.scrollTo).toHaveBeenCalledWith(1800)

    testState.scrollTo.mockClear()
    view.rerender(
      <VirtualizedMessageThread
        items={[{ id: "message-1" }, { id: "message-2" }, { id: "message-3" }]}
        getItemKey={(item) => item.id}
        renderItem={() => <div>message</div>}
        initialViewState={{
          scrollOffset: 1800,
          atBottom: false,
          virtualItemCount: 21,
          virtualizerCache: null,
        }}
      />
    )
    expect(testState.scrollTo).toHaveBeenCalledWith(1800)
  })

  it("publishes the last virtualizer geometry when the surface unmounts", () => {
    const onViewStateChange = vi.fn()
    const view = render(
      <VirtualizedMessageThread
        items={[{ id: "message-1" }]}
        getItemKey={(item) => item.id}
        renderItem={() => <div>message</div>}
        onViewStateChange={onViewStateChange}
      />
    )

    view.unmount()

    expect(onViewStateChange).toHaveBeenCalledWith({
      scrollOffset: 720,
      atBottom: false,
      virtualItemCount: 1,
      virtualizerCache: { measured: true },
    })
  })
})
