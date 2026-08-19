import { describe, expect, it } from "vitest"
import { splitGroup, type LayoutNode } from "./tab-group-layout"
import {
  isMaximizeRestoreEscape,
  shouldExitMaximizedGroup,
} from "./pane-maximize"

const leaf = (id: string): LayoutNode => ({ type: "group", id })

describe("shouldExitMaximizedGroup", () => {
  it("is a no-op when nothing is maximized", () => {
    const tree = leaf("a")
    expect(
      shouldExitMaximizedGroup({
        maximizedGroupId: null,
        prevGroupLayout: tree,
        groupLayout: tree,
        activeGroupId: "a",
      })
    ).toBe(false)
  })

  it("stays maximized while the tree and the active group are unchanged", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    expect(
      shouldExitMaximizedGroup({
        maximizedGroupId: "a",
        prevGroupLayout: two,
        groupLayout: two,
        activeGroupId: "a",
      })
    ).toBe(false)
  })

  it("exits on any new groupLayout reference (split/dissolve/merge/flip/remote sync)", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    // Same shape, but a NEW object identity — exactly what a split/dissolve/
    // remote-sync produces even when it happens not to move the active tab.
    const twoAgain: LayoutNode = { ...two }
    expect(
      shouldExitMaximizedGroup({
        maximizedGroupId: "a",
        prevGroupLayout: two,
        groupLayout: twoAgain,
        activeGroupId: "a",
      })
    ).toBe(true)
  })

  it("exits when the maximized group is no longer a leaf (dissolved/removed)", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const collapsed = leaf("a")
    expect(
      shouldExitMaximizedGroup({
        maximizedGroupId: "b",
        prevGroupLayout: two,
        groupLayout: collapsed,
        activeGroupId: "a",
      })
    ).toBe(true)
  })

  it("exits when the active tab's focus moves to a different, hidden group", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    // Same layout reference (e.g. next/prev-tab moves focus without touching
    // the tree), but the active tab now lives in the OTHER group.
    expect(
      shouldExitMaximizedGroup({
        maximizedGroupId: "a",
        prevGroupLayout: two,
        groupLayout: two,
        activeGroupId: "b",
      })
    ).toBe(true)
  })

  it("stays maximized when there is no active tab at all", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    expect(
      shouldExitMaximizedGroup({
        maximizedGroupId: "a",
        prevGroupLayout: two,
        groupLayout: two,
        activeGroupId: null,
      })
    ).toBe(false)
  })
})

describe("isMaximizeRestoreEscape", () => {
  it("accepts a plain, unhandled Escape with no overlay and no editable focus", () => {
    expect(
      isMaximizeRestoreEscape(
        { key: "Escape", defaultPrevented: false },
        { hasOpenOverlay: false, focusIsEditable: false }
      )
    ).toBe(true)
  })

  it("ignores any key other than Escape", () => {
    expect(
      isMaximizeRestoreEscape(
        { key: "Enter", defaultPrevented: false },
        { hasOpenOverlay: false, focusIsEditable: false }
      )
    ).toBe(false)
  })

  it("defers to a handler that already called preventDefault", () => {
    expect(
      isMaximizeRestoreEscape(
        { key: "Escape", defaultPrevented: true },
        { hasOpenOverlay: false, focusIsEditable: false }
      )
    ).toBe(false)
  })

  it("defers to an open dialog/alertdialog/menu instead of restoring", () => {
    expect(
      isMaximizeRestoreEscape(
        { key: "Escape", defaultPrevented: false },
        { hasOpenOverlay: true, focusIsEditable: false }
      )
    ).toBe(false)
  })

  it("never steals Escape from an editable element (input/textarea/contenteditable)", () => {
    expect(
      isMaximizeRestoreEscape(
        { key: "Escape", defaultPrevented: false },
        { hasOpenOverlay: false, focusIsEditable: true }
      )
    ).toBe(false)
  })
})
