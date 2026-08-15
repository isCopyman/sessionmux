import { describe, it, expect, afterEach } from "vitest"
import { act, render, screen } from "@testing-library/react"
import { TabDragGhost } from "./tab-drag-ghost"
import { useTabStore } from "@/contexts/tab-context"

const dragTo = (
  overGroupId: string | null,
  x = 120,
  y = 64,
  splitEdge: "left" | "right" | "up" | "down" | null = null
) =>
  act(() => {
    useTabStore.getState().updateTabDrag({
      tabId: "conv-1-codex-9",
      title: "Refactor the parser",
      x,
      y,
      overGroupId,
      splitEdge,
    })
  })

const release = () =>
  act(() => {
    useTabStore.getState().endTabDrag()
  })

describe("TabDragGhost", () => {
  afterEach(() => {
    release()
  })

  it("shows the floating duplicate for the whole drag", () => {
    render(<TabDragGhost />)
    expect(screen.queryByText("Refactor the parser")).toBeNull()

    // The source tab stays in place as a dim placeholder while the duplicate
    // follows the cursor, even before a pane target is acquired.
    dragTo(null)
    expect(screen.getByText("Refactor the parser")).toBeTruthy()

    dragTo("g-2")
    expect(screen.getByText("Refactor the parser")).toBeTruthy()

    // Splitting at an edge of the source pane is also a valid drop target.
    dragTo("g-1", 180, 64, "right")
    expect(screen.getByText("Refactor the parser")).toBeTruthy()

    dragTo(null)
    expect(screen.getByText("Refactor the parser")).toBeTruthy()

    release()
    expect(screen.queryByText("Refactor the parser")).toBeNull()
  })

  it("leaves text-selection suppression to the dragged tab itself", () => {
    // The guard covers EVERY tab drag (within-group sorting, the unsplit strip),
    // not just the pane moves that produce a ghost — so it lives in
    // TabItem via drag-selection-guard, not here. See
    // src/lib/drag-selection-guard.test.ts.
    render(<TabDragGhost />)
    dragTo("g-2")
    expect(document.body.classList.contains("select-none")).toBe(false)
  })
})
