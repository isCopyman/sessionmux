import { beforeEach, describe, expect, it, vi } from "vitest"
import type { WorkbenchInfo } from "@/lib/types"

const h = vi.hoisted(() => ({
  listWorkbenches: vi.fn(),
  createWorkbench: vi.fn(),
  deleteWorkbench: vi.fn(),
  duplicateWorkbench: vi.fn(),
  reorderWorkbenches: vi.fn(),
  renameWorkbench: vi.fn(),
  setWorkbenchPinned: vi.fn(),
  switchWorkbench: vi.fn(async () => {}),
  activeWorkbenchId: 1,
}))

vi.mock("@/lib/api", () => ({
  listWorkbenches: h.listWorkbenches,
  createWorkbench: h.createWorkbench,
  deleteWorkbench: h.deleteWorkbench,
  duplicateWorkbench: h.duplicateWorkbench,
  reorderWorkbenches: h.reorderWorkbenches,
  renameWorkbench: h.renameWorkbench,
  setWorkbenchPinned: h.setWorkbenchPinned,
}))

vi.mock("@/stores/tab-store", () => ({
  duplicateWorkbenchLocalState: vi.fn(),
  useTabStore: {
    getState: () => ({
      activeWorkbenchId: h.activeWorkbenchId,
      switchWorkbench: h.switchWorkbench,
      rawTabs: [],
      openChatModeTab: vi.fn(),
      openNewConversationTab: vi.fn(),
    }),
  },
}))

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: {
    getState: () => ({ folders: [], activeFolderId: null }),
  },
}))

import { useWorkbenchStore } from "@/stores/workbench-store"

function workbench(id: number, name: string): WorkbenchInfo {
  return {
    id,
    name,
    position: id - 1,
    is_pinned: false,
    created_at: "2026-08-16T00:00:00Z",
    updated_at: "2026-08-16T00:00:00Z",
  }
}

describe("workbench window views", () => {
  beforeEach(() => {
    localStorage.clear()
    vi.clearAllMocks()
    h.activeWorkbenchId = 1
    h.listWorkbenches.mockResolvedValue([
      workbench(1, "Main"),
      workbench(2, "Review"),
    ])
    useWorkbenchStore.setState({
      items: [],
      openIds: [],
      recentlyClosedIds: [],
      hydrated: false,
      loading: false,
    })
  })

  it("hydrates saved Workbenches separately from locally mounted tabs", async () => {
    await useWorkbenchStore.getState().hydrate()

    expect(useWorkbenchStore.getState().items.map((item) => item.id)).toEqual([
      1, 2,
    ])
    expect(useWorkbenchStore.getState().openIds).toEqual([1, 2])
  })

  it("switches before closing the active view and keeps the Workbench saved", async () => {
    await useWorkbenchStore.getState().hydrate()
    await useWorkbenchStore.getState().closeView(1)

    expect(h.switchWorkbench).toHaveBeenCalledWith(2)
    expect(useWorkbenchStore.getState().items).toHaveLength(2)
    expect(useWorkbenchStore.getState().openIds).toEqual([2])
    expect(useWorkbenchStore.getState().recentlyClosedIds).toEqual([1])
    expect(h.deleteWorkbench).not.toHaveBeenCalled()
  })

  it("keeps at least one view mounted and can reopen a closed Workbench", async () => {
    await useWorkbenchStore.getState().hydrate()
    await useWorkbenchStore.getState().closeView(2)
    await useWorkbenchStore.getState().closeView(1)

    expect(useWorkbenchStore.getState().openIds).toEqual([1])
    expect(useWorkbenchStore.getState().recentlyClosedIds).toEqual([2])

    await useWorkbenchStore.getState().reopenAndSwitch(2)
    expect(useWorkbenchStore.getState().openIds).toEqual([1, 2])
    expect(useWorkbenchStore.getState().recentlyClosedIds).toEqual([])
    expect(h.switchWorkbench).toHaveBeenCalledWith(2)
  })
})
