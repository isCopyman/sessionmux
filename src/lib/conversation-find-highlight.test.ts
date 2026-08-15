import { describe, expect, it, vi } from "vitest"
import {
  applyConversationFindHighlights,
  findTextRanges,
  revealConversationFindRange,
} from "@/lib/conversation-find-highlight"

describe("conversation find DOM ranges", () => {
  it("maps a match that crosses adjacent rendered text nodes", () => {
    const root = document.createElement("div")
    root.innerHTML = "hello <strong>world</strong> again"

    const ranges = findTextRanges(root, "o wo")

    expect(ranges).toHaveLength(1)
    expect(ranges[0].toString()).toBe("o wo")
  })

  it("excludes transcript chrome explicitly marked as non-searchable", () => {
    const root = document.createElement("div")
    root.innerHTML =
      "<span>answer</span><button data-conversation-search-exclude>answer</button>"

    expect(findTextRanges(root, "answer")).toHaveLength(1)
  })

  it("injects highlight styles once outside the compiled stylesheet", () => {
    const root = document.createElement("div")
    root.innerHTML = `
      <div data-virtual-item-index="0">
        <div data-conversation-search-content>Find this text</div>
      </div>
    `
    document.body.append(root)
    const registry = { set: vi.fn(), delete: vi.fn() }
    class TestHighlight {}
    Object.defineProperty(globalThis.CSS, "highlights", {
      configurable: true,
      value: registry,
    })
    Object.defineProperty(globalThis, "Highlight", {
      configurable: true,
      value: TestHighlight,
    })

    applyConversationFindHighlights(root, "Find", {
      threadIndex: 0,
      occurrenceIndex: 0,
    })
    applyConversationFindHighlights(root, "text", {
      threadIndex: 0,
      occurrenceIndex: 0,
    })

    const styles = document.querySelectorAll("#codeg-conversation-find-styles")
    expect(styles).toHaveLength(1)
    expect(styles[0].textContent).toContain(
      "::highlight(codeg-conversation-find-match)"
    )
    expect(registry.set).toHaveBeenCalled()
  })

  it("returns the exact selected occurrence inside a long rendered row", () => {
    const root = document.createElement("div")
    root.innerHTML = `
      <div data-virtual-item-index="7">
        <div data-conversation-search-content>first word, second word</div>
      </div>
    `

    const range = applyConversationFindHighlights(root, "word", {
      threadIndex: 7,
      occurrenceIndex: 1,
    })

    expect(range?.toString()).toBe("word")
    expect(root.querySelector("[data-virtual-item-index='7']")).toHaveAttribute(
      "data-conversation-find-current"
    )
  })

  it("keeps a fully visible selected range in place", () => {
    const root = document.createElement("div")
    const viewport = document.createElement("div")
    viewport.className = "scrollbar-thin"
    viewport.scrollTop = 120
    root.append(viewport)
    const scrollTo = vi.fn()
    viewport.scrollTo = scrollTo
    vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
      top: 100,
      height: 600,
    } as DOMRect)
    const range = document.createRange()
    Object.defineProperty(range, "getClientRects", {
      configurable: true,
      value: () => [{ top: 300, height: 20, width: 35 }],
    })

    expect(revealConversationFindRange(root, range)).toBe(true)
    expect(scrollTo).not.toHaveBeenCalled()
  })

  it("centers a selected range outside the transcript viewport", () => {
    const root = document.createElement("div")
    const viewport = document.createElement("div")
    viewport.className = "scrollbar-thin"
    viewport.scrollTop = 120
    root.append(viewport)
    const scrollTo = vi.fn()
    viewport.scrollTo = scrollTo
    vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
      top: 100,
      height: 600,
    } as DOMRect)
    const range = document.createRange()
    Object.defineProperty(range, "getClientRects", {
      configurable: true,
      value: () => [{ top: 730, height: 20, width: 35 }],
    })

    expect(revealConversationFindRange(root, range)).toBe(true)
    expect(scrollTo).toHaveBeenCalledWith({
      top: 460,
      behavior: "auto",
    })
  })
})
