import { act, cleanup, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => {
  const workspace = {
    foldersHydrated: true,
    folders: [{ id: 7 }],
    addFolderToWorkspaceById: vi.fn(),
  }
  const tabs = {
    tabsHydrated: true,
    activeWorkbenchId: 3,
    rawTabs: [
      {
        id: "session-11",
        kind: "conversation",
        folderId: 7,
        conversationId: 11,
      },
      {
        id: "session-12",
        kind: "conversation",
        folderId: 7,
        conversationId: 12,
      },
    ],
    groupOf: {},
    groupLayout: {},
    switchWorkbench: vi.fn(),
    openTab: vi.fn(),
    splitTab: vi.fn(),
    switchTab: vi.fn(),
  }
  const workbenches = {
    items: [{ id: 3 }],
    hydrate: vi.fn(),
    ensureOpen: vi.fn(),
  }
  return {
    workspace,
    tabs,
    workbenches,
    handler: null as null | ((payload: unknown) => void),
  }
})

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: Object.assign(
    (selector: (state: typeof mocks.workspace) => unknown) =>
      selector(mocks.workspace),
    { getState: () => mocks.workspace }
  ),
}))

vi.mock("@/stores/tab-store", () => ({
  groupOfTab: () => "group-a",
  useTabStore: Object.assign(
    (selector: (state: typeof mocks.tabs) => unknown) => selector(mocks.tabs),
    { getState: () => mocks.tabs }
  ),
}))

vi.mock("@/stores/workbench-store", () => ({
  useWorkbenchStore: Object.assign(() => mocks.workbenches, {
    getState: () => mocks.workbenches,
  }),
}))

vi.mock("@/lib/platform", () => ({
  subscribe: async (_event: string, handler: (payload: unknown) => void) => {
    mocks.handler = handler
    return () => {}
  },
}))

import { HostControlWorkbenchBridge } from "./host-control-workbench-bridge"

describe("HostControlWorkbenchBridge", () => {
  beforeEach(() => {
    mocks.handler = null
    vi.clearAllMocks()
  })
  afterEach(() => cleanup())

  it("uses the existing Pane split primitive for an explicit placement", async () => {
    render(<HostControlWorkbenchBridge />)
    await waitFor(() => expect(mocks.handler).toBeTruthy())

    act(() => {
      mocks.handler!({
        requestId: "place-1",
        workbenchId: 3,
        folderId: 7,
        conversationId: 12,
        agent: "codex",
        placement: "right",
      })
    })

    await waitFor(() =>
      expect(mocks.tabs.splitTab).toHaveBeenCalledWith("session-12", "right", {
        move: true,
      })
    )
    expect(mocks.workbenches.ensureOpen).toHaveBeenCalledWith(3)
    expect(mocks.tabs.switchWorkbench).not.toHaveBeenCalled()
  })
})
