import { useRef, useState } from "react"
import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
} from "@testing-library/react"
import { describe, expect, it } from "vitest"

import {
  useTranscriptFind,
  type TranscriptFindEntry,
} from "./use-transcript-find"

const ENTRIES: TranscriptFindEntry[] = [
  { key: "a", index: 0, text: "alpha" },
  { key: "b", index: 2, text: "needle in hay" },
  { key: "c", index: 5, text: "another needle" },
]

function openFind() {
  fireEvent.keyDown(document.body, { key: "f", ctrlKey: true, bubbles: true })
}

describe("useTranscriptFind", () => {
  it("opens on Ctrl/Cmd+F and reports match counts", () => {
    const rootRef = { current: document.createElement("div") }
    const { result } = renderHook(() =>
      useTranscriptFind({
        entries: ENTRIES,
        isActive: true,
        rootRef,
      })
    )
    expect(result.current.open).toBe(false)

    act(() => {
      openFind()
    })
    expect(result.current.open).toBe(true)
    expect(result.current.focusToken).toBeGreaterThan(0)

    act(() => {
      result.current.onQueryChange("needle")
    })
    expect(result.current.total).toBe(2)
    expect(result.current.current).toBe(1)
    expect(result.current.query).toBe("needle")
  })

  it("wraps next and previous around the match list", () => {
    const rootRef = { current: document.createElement("div") }
    const { result } = renderHook(() =>
      useTranscriptFind({
        entries: ENTRIES,
        isActive: true,
        rootRef,
      })
    )
    act(() => {
      openFind()
      result.current.onQueryChange("needle")
    })
    act(() => {
      result.current.onNext()
    })
    expect(result.current.current).toBe(2)
    act(() => {
      result.current.onNext()
    })
    expect(result.current.current).toBe(1)
    act(() => {
      result.current.onPrevious()
    })
    expect(result.current.current).toBe(2)
  })

  it("resets query and selection on close", () => {
    const rootRef = { current: document.createElement("div") }
    const { result } = renderHook(() =>
      useTranscriptFind({
        entries: ENTRIES,
        isActive: true,
        rootRef,
      })
    )
    act(() => {
      openFind()
      result.current.onQueryChange("needle")
    })
    act(() => {
      result.current.onClose()
    })
    expect(result.current.open).toBe(false)
    expect(result.current.query).toBe("")
    expect(result.current.total).toBe(0)
    expect(result.current.current).toBe(0)
  })

  it("does not steal Ctrl+F from a native-find scope", () => {
    const rootRef = { current: document.createElement("div") }
    const { result } = renderHook(() =>
      useTranscriptFind({
        entries: ENTRIES,
        isActive: true,
        rootRef,
      })
    )
    const editor = document.createElement("div")
    editor.className = "monaco-editor"
    document.body.append(editor)
    fireEvent.keyDown(editor, { key: "f", ctrlKey: true, bubbles: true })
    expect(result.current.open).toBe(false)
    editor.remove()
  })

  it("does not arm Ctrl+F when isActive is false", () => {
    const rootRef = { current: document.createElement("div") }
    const { result } = renderHook(() =>
      useTranscriptFind({
        entries: ENTRIES,
        isActive: false,
        rootRef,
      })
    )
    act(() => {
      openFind()
    })
    expect(result.current.open).toBe(false)
  })

  it("treats entry.index as the virtualization display index, not the entries-array offset", () => {
    function Probe() {
      const rootRef = useRef<HTMLDivElement>(null)
      const [queryArmed, setQueryArmed] = useState(false)
      const find = useTranscriptFind({
        entries: ENTRIES,
        isActive: true,
        rootRef,
        scrollToIndex: (index) => {
          setQueryArmed(true)
          document
            .getElementById("probe-index")
            ?.setAttribute("data-scrolled-to", String(index))
        },
      })
      return (
        <div ref={rootRef}>
          <button type="button" onClick={() => find.onQueryChange("needle")}>
            search
          </button>
          <span id="probe-index" data-scrolled-to="" />
          <span data-testid="armed">{String(queryArmed)}</span>
        </div>
      )
    }

    render(<Probe />)
    act(() => {
      openFind()
    })
    fireEvent.click(screen.getByText("search"))
    expect(screen.getByTestId("armed")).toHaveTextContent("true")
    expect(document.getElementById("probe-index")).toHaveAttribute(
      "data-scrolled-to",
      "2"
    )
  })
})
