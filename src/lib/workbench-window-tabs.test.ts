import { beforeEach, describe, expect, it } from "vitest"
import {
  closeWorkbenchWindowTab,
  loadWorkbenchWindowTabs,
  openWorkbenchWindowTab,
  saveWorkbenchWindowTabs,
  WORKBENCH_WINDOW_TABS_STORAGE_KEY,
} from "@/lib/workbench-window-tabs"

describe("Workbench window tabs", () => {
  beforeEach(() => localStorage.clear())

  it("opens every saved Workbench when no window preference exists", () => {
    expect(loadWorkbenchWindowTabs([1, 2, 3], 2)).toEqual({
      openIds: [1, 2, 3],
      recentlyClosedIds: [],
    })
  })

  it("repairs stale ids and always includes the active Workbench", () => {
    localStorage.setItem(
      WORKBENCH_WINDOW_TABS_STORAGE_KEY,
      JSON.stringify({
        openIds: [1, 99, 1],
        recentlyClosedIds: [2, 99, 1],
      })
    )

    expect(loadWorkbenchWindowTabs([1, 2, 3], 3)).toEqual({
      openIds: [1, 3],
      recentlyClosedIds: [2],
    })
  })

  it("closes only the local view and keeps a bounded recent stack", () => {
    const closed = closeWorkbenchWindowTab(
      { openIds: [1, 2], recentlyClosedIds: [3] },
      2
    )
    expect(closed).toEqual({
      openIds: [1],
      recentlyClosedIds: [2, 3],
    })
    expect(closeWorkbenchWindowTab(closed, 1)).toBe(closed)
  })

  it("reopens a tab, removes it from recent, and persists the result", () => {
    const reopened = openWorkbenchWindowTab(
      { openIds: [1], recentlyClosedIds: [2, 3] },
      2
    )
    saveWorkbenchWindowTabs(reopened)

    expect(loadWorkbenchWindowTabs([1, 2, 3], 2)).toEqual({
      openIds: [1, 2],
      recentlyClosedIds: [3],
    })
  })
})
