import { useMemo, useRef, useState } from "react"
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { useTranscriptFind } from "./use-transcript-find"
import { useTranscriptInfiniteScroll } from "./use-transcript-infinite-scroll"

type Row = { id: string; text: string }

function openFind() {
  fireEvent.keyDown(document.body, { key: "f", ctrlKey: true, bubbles: true })
}

/**
 * Windowed transcript stand-in: only `visible` display-order indices exist in
 * the DOM (the rest are virtualized away). `scrollToIndex` mounts the target
 * row, matching VirtualizedTranscript's off-screen contract.
 */
function OffScreenFindHarness({
  items,
  visible: visibleProp,
  hasOlder = false,
  isLoading = false,
  onLoadOlder,
}: {
  items: Row[]
  visible: ReadonlySet<number>
  hasOlder?: boolean
  isLoading?: boolean
  onLoadOlder?: () => void
}) {
  const [mounted, setMounted] = useState<Set<number>>(
    () => new Set(visibleProp)
  )
  const rootRef = useRef<HTMLDivElement>(null)
  const entries = useMemo(
    () =>
      items.map((item, index) => ({
        key: item.id,
        index,
        text: item.text,
      })),
    [items]
  )
  const scrollToIndex = vi.fn((index: number) => {
    setMounted((prev) => {
      const next = new Set(prev)
      next.add(index)
      return next
    })
  })
  const find = useTranscriptFind({
    entries,
    isActive: true,
    rootRef,
    scrollToIndex,
  })
  const infiniteScroll = useTranscriptInfiniteScroll({
    hasOlder,
    isLoading,
    onLoadOlder: onLoadOlder ?? (() => undefined),
    isActive: true,
    findOpen: find.open,
    findQuery: find.query,
  })

  return (
    <div>
      <div data-testid="find-total">{find.total}</div>
      <div data-testid="find-current">{find.current}</div>
      <div data-testid="searching">
        {String(infiniteScroll.searchingOlderHistory)}
      </div>
      <button
        type="button"
        data-testid="search-needle"
        onClick={() => find.onQueryChange("needle")}
      >
        search
      </button>
      <div ref={rootRef} data-testid="transcript-root">
        <div className="scrollbar-thin" data-testid="viewport" />
        {items.map((item, index) =>
          mounted.has(index) ? (
            <div key={item.id} data-virtual-item-index={index}>
              <div data-conversation-search-content>{item.text}</div>
            </div>
          ) : null
        )}
      </div>
    </div>
  )
}

describe("off-screen find locate + highlight", () => {
  let highlightRegistry: {
    set: ReturnType<typeof vi.fn>
    delete: ReturnType<typeof vi.fn>
  }

  beforeEach(() => {
    highlightRegistry = { set: vi.fn(), delete: vi.fn() }
    class TestHighlight {}
    Object.defineProperty(globalThis.CSS, "highlights", {
      configurable: true,
      value: highlightRegistry,
    })
    Object.defineProperty(globalThis, "Highlight", {
      configurable: true,
      value: TestHighlight,
    })
  })

  afterEach(() => {
    document.getElementById("codeg-conversation-find-styles")?.remove()
  })

  it("scrolls an off-screen hit into view and paints the current highlight", async () => {
    const items: Row[] = [
      { id: "0", text: "alpha" },
      { id: "1", text: "the needle is here" },
      { id: "2", text: "omega" },
    ]
    render(<OffScreenFindHarness items={items} visible={new Set([2])} />)

    expect(document.querySelector("[data-virtual-item-index='1']")).toBeNull()

    act(() => {
      openFind()
    })
    fireEvent.click(screen.getByTestId("search-needle"))

    await waitFor(() => {
      expect(screen.getByTestId("find-total")).toHaveTextContent("1")
    })
    await waitFor(() => {
      const row = document.querySelector("[data-virtual-item-index='1']")
      expect(row).not.toBeNull()
      expect(row).toHaveAttribute("data-conversation-find-current")
    })
    expect(highlightRegistry.set).toHaveBeenCalled()
  })

  it("keeps entry.index equal to the virtualization display index after prepend", async () => {
    const tail: Row[] = [
      { id: "a", text: "alpha" },
      { id: "needle-row", text: "the needle is here" },
      { id: "z", text: "omega" },
    ]
    const { rerender } = render(
      <OffScreenFindHarness items={tail} visible={new Set([2])} />
    )

    act(() => {
      openFind()
    })
    fireEvent.click(screen.getByTestId("search-needle"))

    await waitFor(() => {
      expect(
        document.querySelector("[data-virtual-item-index='1']")
      ).toHaveAttribute("data-conversation-find-current")
    })

    const prepended: Row[] = [
      { id: "older", text: "older history without the word" },
      ...tail,
    ]
    rerender(
      <OffScreenFindHarness items={prepended} visible={new Set([0, 2, 3])} />
    )

    await waitFor(() => {
      const row = document.querySelector("[data-virtual-item-index='2']")
      expect(row).toHaveTextContent("the needle is here")
      expect(row).toHaveAttribute("data-conversation-find-current")
    })
    expect(
      document.querySelector("[data-virtual-item-index='1']")
    ).not.toHaveAttribute("data-conversation-find-current")
  })

  it("keeps searchingOlderHistory true until older pages are exhausted", () => {
    const items: Row[] = [{ id: "z", text: "omega needle" }]
    const onLoadOlder = vi.fn()
    const { rerender } = render(
      <OffScreenFindHarness
        items={items}
        visible={new Set([0])}
        hasOlder
        onLoadOlder={onLoadOlder}
      />
    )
    act(() => {
      openFind()
    })
    fireEvent.click(screen.getByTestId("search-needle"))
    expect(screen.getByTestId("searching")).toHaveTextContent("true")
    expect(onLoadOlder).toHaveBeenCalled()

    rerender(
      <OffScreenFindHarness
        items={[
          { id: "older", text: "older needle" },
          { id: "z", text: "omega needle" },
        ]}
        visible={new Set([0, 1])}
        hasOlder={false}
        onLoadOlder={onLoadOlder}
      />
    )
    expect(screen.getByTestId("searching")).toHaveTextContent("false")
  })
})
