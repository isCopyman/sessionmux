import { act, render, screen, waitFor } from "@testing-library/react"
import { createRef } from "react"
import { describe, expect, it } from "vitest"

import { RichComposer, type RichComposerHandle } from "./rich-composer"
import type { ReferenceSearch } from "./suggestion/types"

/**
 * Answers every query with a single row named after it, and records what it was
 * asked. Both halves matter here: the row proves which query the panel is
 * showing, the log proves which queries it searched for.
 */
function trackingSearch() {
  const queries: string[] = []
  const search: ReferenceSearch = (query) => {
    queries.push(query)
    return [
      {
        kind: "file",
        label: "Files",
        items: [
          {
            reference: {
              refType: "file",
              id: `${query}.ts`,
              label: `${query}.ts`,
              uri: `file:///repo/${query}.ts`,
              meta: { fileKind: "file" },
            },
          },
        ],
      },
    ]
  }
  return { queries, search }
}

async function mount(search: ReferenceSearch) {
  const ref = createRef<RichComposerHandle>()
  render(<RichComposer ref={ref} referenceSearch={search} />)
  await waitFor(() => expect(ref.current?.getEditor()).not.toBeNull(), {
    timeout: 5000,
  })
  const editor = ref.current?.getEditor()
  if (!editor) throw new Error("editor not mounted")
  return editor
}

/**
 * jsdom has no input method, so a composition is staged by hand: the DOM events
 * ProseMirror listens for, with the document edits the IME would have made in
 * between. A plain `Event` is enough — ProseMirror reads only `timeStamp` off
 * these two.
 */
const compositionEvent = (type: "compositionstart" | "compositionend") =>
  new Event(type, { bubbles: true, cancelable: true })

/** Longer than the panel's 150ms fetch debounce. */
const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 300))
  })

describe("RichComposer @ mention under an IME", () => {
  it("keeps the panel open through a composition and refreshes on commit", async () => {
    const { queries, search } = trackingSearch()
    const editor = await mount(search)
    const dom = editor.view.dom as HTMLElement

    // `@美` is typed and confirmed the ordinary way.
    act(() => {
      editor.commands.insertContent("@美")
    })
    await screen.findByText("美.ts", {}, { timeout: 5000 })

    // The second character goes through the IME: its romaji lands in the
    // document while the composition is still in flight.
    act(() => {
      dom.dispatchEvent(compositionEvent("compositionstart"))
    })
    expect(editor.view.composing).toBe(true)
    act(() => {
      editor.commands.insertContent("shu")
    })
    await settle()

    // This is issue #518: the panel used to vanish here, and the query with it.
    expect(screen.getByTestId("mention-popup")).toBeInTheDocument()
    expect(screen.getByText("美.ts")).toBeInTheDocument()
    expect(queries).not.toContain("美shu")

    // The IME swaps the romaji for the chosen character and commits.
    act(() => {
      const at = editor.state.selection.from
      editor.commands.deleteRange({ from: at - 3, to: at })
      editor.commands.insertContent("术")
      dom.dispatchEvent(compositionEvent("compositionend"))
    })

    await screen.findByText("美术.ts", {}, { timeout: 5000 })
    expect(queries).toContain("美术")
  })

  it("returns to the committed query when the composition is cancelled", async () => {
    const { search } = trackingSearch()
    const editor = await mount(search)
    const dom = editor.view.dom as HTMLElement

    act(() => {
      editor.commands.insertContent("@美")
    })
    await screen.findByText("美.ts", {}, { timeout: 5000 })

    act(() => {
      dom.dispatchEvent(compositionEvent("compositionstart"))
    })
    act(() => {
      editor.commands.insertContent("shu")
    })
    // Escape: the IME takes its own text back out before ending the
    // composition, leaving the document as it was.
    act(() => {
      const at = editor.state.selection.from
      editor.commands.deleteRange({ from: at - 3, to: at })
      dom.dispatchEvent(compositionEvent("compositionend"))
    })
    await settle()

    expect(screen.getByTestId("mention-popup")).toBeInTheDocument()
    expect(screen.getByText("美.ts")).toBeInTheDocument()
    expect(editor.getText()).toContain("@美")
  })

  it("defers a trigger typed inside a composition until it commits", async () => {
    const { search } = trackingSearch()
    const editor = await mount(search)
    const dom = editor.view.dom as HTMLElement

    act(() => {
      dom.dispatchEvent(compositionEvent("compositionstart"))
    })
    act(() => {
      editor.commands.insertContent("@美")
    })
    await settle()

    // Nothing is committed yet, so the half of the old guard that was worth
    // keeping still holds: no panel opens off uncommitted text.
    expect(screen.queryByTestId("mention-popup")).toBeNull()

    act(() => {
      dom.dispatchEvent(compositionEvent("compositionend"))
    })
    await screen.findByText("美.ts", {}, { timeout: 5000 })
  })
})
