import { beforeEach, describe, expect, it } from "vitest"
import { singleGroupLayout, splitGroup } from "@/lib/tab-group-layout"
import { resetTabStore, useTabStore } from "./tab-store"
import type { TabItemInternal } from "./tab-store"

function sessionTab(id: number): TabItemInternal {
  return {
    id: `conv-1-codex-${id}`,
    kind: "conversation",
    folderId: 1,
    conversationId: id,
    agentType: "codex",
    title: `Session ${id}`,
    isPinned: true,
  }
}

describe("group maximize (pane zoom)", () => {
  beforeEach(() => {
    localStorage.clear()
    resetTabStore()
  })

  it("does nothing while unsplit — a single group already fills the area", () => {
    const tabA = sessionTab(1)
    useTabStore.setState({
      rawTabs: [tabA],
      activeTabId: tabA.id,
      groupLayout: singleGroupLayout("g-main"),
      groupOf: {},
      groupSelection: {},
    })

    useTabStore.getState().toggleGroupMaximized("g-main")

    expect(useTabStore.getState().maximizedGroupId).toBeNull()
  })

  it("maximizes an already-focused split group, leaving the active tab untouched", () => {
    const tabA = sessionTab(1)
    const tabB = sessionTab(2)
    const layout = splitGroup(singleGroupLayout("a"), "a", "right", "b")
    useTabStore.setState({
      rawTabs: [tabA, tabB],
      activeTabId: tabB.id,
      groupLayout: layout,
      groupOf: { [tabA.id]: "a", [tabB.id]: "b" },
      groupSelection: { a: tabA.id, b: tabB.id },
    })

    useTabStore.getState().toggleGroupMaximized("b")

    expect(useTabStore.getState().maximizedGroupId).toBe("b")
    expect(useTabStore.getState().activeTabId).toBe(tabB.id)
  })

  it("maximizing a non-focused sibling pane also focuses its own selected tab", () => {
    const tabA = sessionTab(1)
    const tabB = sessionTab(2)
    const layout = splitGroup(singleGroupLayout("a"), "a", "right", "b")
    useTabStore.setState({
      rawTabs: [tabA, tabB],
      activeTabId: tabA.id,
      groupLayout: layout,
      groupOf: { [tabA.id]: "a", [tabB.id]: "b" },
      groupSelection: { a: tabA.id, b: tabB.id },
    })

    useTabStore.getState().toggleGroupMaximized("b")

    expect(useTabStore.getState().maximizedGroupId).toBe("b")
    // Without this, the active tab would still point at hidden group "a" —
    // the auto-exit guard (shouldExitMaximizedGroup) would then immediately
    // clear the maximize flag that was just set.
    expect(useTabStore.getState().activeTabId).toBe(tabB.id)
  })

  it("toggling the already-maximized group restores the normal split", () => {
    const tabA = sessionTab(1)
    const tabB = sessionTab(2)
    const layout = splitGroup(singleGroupLayout("a"), "a", "right", "b")
    useTabStore.setState({
      rawTabs: [tabA, tabB],
      activeTabId: tabB.id,
      groupLayout: layout,
      groupOf: { [tabA.id]: "a", [tabB.id]: "b" },
      groupSelection: { a: tabA.id, b: tabB.id },
      maximizedGroupId: "b",
    })

    useTabStore.getState().toggleGroupMaximized("b")

    expect(useTabStore.getState().maximizedGroupId).toBeNull()
  })

  it("an outer divider drag does not re-anchor (nor un-zoom) a maximized pane", () => {
    const tabA = sessionTab(1)
    const tabB = sessionTab(2)
    const layout = splitGroup(singleGroupLayout("a"), "a", "right", "b")
    useTabStore.setState({
      rawTabs: [tabA, tabB],
      activeTabId: tabB.id,
      groupLayout: layout,
      groupOf: { [tabA.id]: "a", [tabB.id]: "b" },
      groupSelection: { a: tabA.id, b: tabB.id },
      maximizedGroupId: "b",
    })

    useTabStore.getState().reanchorGroupSplits("horizontal", "end", 300 / 240)

    // A new groupLayout reference would trip shouldExitMaximizedGroup and drop
    // the zoom mid-drag; while zoomed there are no visible panes to re-anchor.
    expect(useTabStore.getState().groupLayout).toBe(layout)
    expect(useTabStore.getState().maximizedGroupId).toBe("b")
  })

  it("exitGroupMaximize unconditionally clears, and no-ops when already clear", () => {
    useTabStore.setState({ maximizedGroupId: "a" })

    useTabStore.getState().exitGroupMaximize()
    expect(useTabStore.getState().maximizedGroupId).toBeNull()

    expect(() => useTabStore.getState().exitGroupMaximize()).not.toThrow()
    expect(useTabStore.getState().maximizedGroupId).toBeNull()
  })
})
