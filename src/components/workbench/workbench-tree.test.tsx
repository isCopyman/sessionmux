import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { WorkbenchTree } from "./workbench-tree"
import enMessages from "@/i18n/messages/en.json"

const h = vi.hoisted(() => ({
  listOpenedTabs: vi.fn(),
  listWorkbenchTabs: vi.fn(),
  hydrate: vi.fn(),
  createAndSwitch: vi.fn(),
  createOnly: vi.fn(),
  duplicateAndSwitch: vi.fn(),
  rename: vi.fn(),
  setPinned: vi.fn(),
  remove: vi.fn(),
  closeView: vi.fn(),
  switchWorkbench: vi.fn(),
  switchTab: vi.fn(),
  openTab: vi.fn(),
  openConversations: vi.fn(),
  openRoom: vi.fn(),
  appendConversationsToWorkbench: vi.fn(),
  rooms: [] as Array<{
    id: string
    workbenchId: number
    title: string
    createdByConversationId: number
    memberCount: number
    unreadCount: number
    updatedAt: string
    createdAt: string
  }>,
  workbenches: [
    {
      id: 1,
      name: "Main",
      position: 0,
      is_pinned: false,
      created_at: "2026-08-01T00:00:00.000Z",
      updated_at: "2026-08-01T00:00:00.000Z",
    },
    {
      id: 2,
      name: "Review",
      position: 1,
      is_pinned: false,
      created_at: "2026-08-02T00:00:00.000Z",
      updated_at: "2026-08-02T00:00:00.000Z",
    },
  ],
  tabState: {
    activeWorkbenchId: 1,
    switchingWorkbench: false,
    tabsHydrated: true,
    tabs: [
      {
        id: "conversation:101",
        kind: "conversation",
        folderId: 7,
        conversationId: 101,
        agentType: "codex",
        title: "Evidence review",
        isPinned: true,
        status: "in_progress",
      },
      {
        id: "conversation:103",
        kind: "conversation",
        folderId: 7,
        conversationId: 103,
        agentType: "codex",
        title: "Fresh result",
        isPinned: true,
        status: "completed",
      },
    ],
    activeTabId: "conversation:101",
    switchWorkbench: vi.fn(),
    switchTab: vi.fn(),
    openTab: vi.fn(),
    closeTab: vi.fn(),
  },
  conversations: [
    {
      id: 101,
      folder_id: 7,
      title: "Evidence review",
      title_locked: true,
      agent_type: "codex",
      status: "in_progress",
      kind: "regular",
      model: null,
      git_branch: null,
      external_id: "session-101",
      message_count: 7,
      child_count: 0,
      created_at: "2026-08-01T00:00:00.000Z",
      updated_at: "2026-08-03T00:00:00.000Z",
      archived_at: null,
      pinned_at: null,
    },
    {
      id: 103,
      folder_id: 7,
      title: "Fresh result",
      title_locked: true,
      agent_type: "codex",
      status: "completed",
      kind: "regular",
      model: null,
      git_branch: null,
      external_id: "session-103",
      message_count: 2,
      child_count: 0,
      created_at: "2026-08-04T00:00:00.000Z",
      updated_at: "2026-08-05T00:00:00.000Z",
      archived_at: null,
      pinned_at: null,
    },
    {
      id: 102,
      folder_id: 9,
      title: "Reviewer session",
      title_locked: true,
      agent_type: "claude",
      status: "completed",
      kind: "regular",
      model: null,
      git_branch: null,
      external_id: "session-102",
      message_count: 4,
      child_count: 0,
      created_at: "2026-08-02T00:00:00.000Z",
      updated_at: "2026-08-02T00:00:00.000Z",
      archived_at: null,
      pinned_at: null,
    },
  ],
}))

vi.mock("sonner", () => ({ toast: { error: vi.fn() } }))

vi.mock("@/lib/api", () => ({
  listOpenedTabs: h.listOpenedTabs,
  listWorkbenchTabs: h.listWorkbenchTabs,
  listCollaborationRooms: vi.fn().mockResolvedValue([]),
}))

vi.mock("@/lib/open-room", () => ({
  useOpenRoom: () => h.openRoom,
}))

vi.mock("@/lib/workbench-session-tabs", () => ({
  appendConversationsToWorkbench: h.appendConversationsToWorkbench,
  SIDEBAR_BULK_TAB_ORIGIN: "sidebar-bulk",
}))

vi.mock("@/stores/room-catalog-store", () => {
  const refresh = vi.fn()
  const state = { rooms: h.rooms, hydrated: true, refresh }
  const useRoomCatalogStore = (selector: (value: typeof state) => unknown) =>
    selector(state)
  useRoomCatalogStore.getState = () => state
  return {
    useRoomCatalogStore,
    ensureRoomCatalogSubscription: vi.fn(),
  }
})

vi.mock("@/stores/workbench-store", () => ({
  useWorkbenchStore: (selector: (state: unknown) => unknown) =>
    selector({
      items: h.workbenches,
      openIds: [1, 2],
      hydrated: true,
      loading: false,
      hydrate: h.hydrate,
      createAndSwitch: h.createAndSwitch,
      createOnly: h.createOnly,
      duplicateAndSwitch: h.duplicateAndSwitch,
      rename: h.rename,
      setPinned: h.setPinned,
      remove: h.remove,
      closeView: h.closeView,
    }),
}))

vi.mock("@/contexts/tab-context", () => ({
  useTabStore: (selector: (state: unknown) => unknown) => selector(h.tabState),
}))

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: unknown) => unknown) =>
    selector({ conversations: h.conversations }),
}))

vi.mock("@/contexts/workbench-route-context", () => ({
  useWorkbenchRoute: () => ({ openConversations: h.openConversations }),
}))

describe("WorkbenchTree", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.rooms.length = 0
    h.appendConversationsToWorkbench.mockResolvedValue({ added: 1, skipped: 0 })
    h.createOnly.mockResolvedValue({
      id: 3,
      name: "Workbench 3",
      position: 2,
      is_pinned: false,
      created_at: "2026-08-07T00:00:00.000Z",
      updated_at: "2026-08-07T00:00:00.000Z",
    })
    h.listOpenedTabs.mockResolvedValue({ items: [], version: 1 })
    h.listWorkbenchTabs.mockResolvedValue({
      items: [
        {
          id: 22,
          folder_id: 9,
          conversation_id: 102,
          agent_type: "claude",
          position: 0,
          is_active: true,
          is_pinned: true,
        },
      ],
      version: 2,
    })
  })

  it("shows Workbenches as roots and their Sessions as children", async () => {
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )

    expect(screen.getByText("Workbenches")).toBeTruthy()
    expect(await screen.findByText("Evidence review")).toBeTruthy()

    await waitFor(() => expect(h.listWorkbenchTabs).toHaveBeenCalledWith(2))
    fireEvent.click(screen.getByRole("button", { name: "Expand workbench" }))
    expect(await screen.findByText("Reviewer session")).toBeTruthy()
  })

  it("orders Workbench navigation by Session activity, not pane tab order", async () => {
    const { container } = render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )

    // Let the inactive-workbench snapshot effect settle so it cannot update
    // the component after this assertion/test has already completed.
    await waitFor(() => expect(h.listWorkbenchTabs).toHaveBeenCalledWith(2))

    const activeRows = Array.from(
      container.querySelectorAll(
        '[data-workbench-id="1"][data-workbench-session]'
      )
    )
    expect(activeRows.map((row) => row.lastElementChild?.textContent)).toEqual([
      "Fresh result",
      "Evidence review",
    ])
    expect(
      container.querySelector('[data-focused-session="true"]')?.textContent
    ).toContain("Evidence review")
  })

  it("switches Workbench before focusing one of its Sessions", async () => {
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )

    await waitFor(() => expect(h.listWorkbenchTabs).toHaveBeenCalledWith(2))
    fireEvent.click(screen.getByRole("button", { name: "Expand workbench" }))
    fireEvent.click(
      await screen.findByRole("button", { name: "Reviewer session" })
    )

    await waitFor(() => {
      expect(h.tabState.switchWorkbench).toHaveBeenCalledWith(2)
      expect(h.tabState.openTab).toHaveBeenCalledWith(
        9,
        102,
        "claude",
        true,
        "Reviewer session"
      )
      expect(h.openConversations).toHaveBeenCalled()
    })
  })

  it("lists Rooms next to Sessions and opens them as Room tabs", async () => {
    h.rooms.push({
      id: "rm_plan",
      workbenchId: 1,
      title: "Plan room",
      createdByConversationId: 101,
      memberCount: 2,
      unreadCount: 0,
      createdAt: "2026-08-06T00:00:00.000Z",
      updatedAt: "2026-08-06T00:00:00.000Z",
    })
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )
    expect(await screen.findByText("Plan room")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Plan room" }))
    await waitFor(() => {
      expect(h.openRoom).toHaveBeenCalledWith(
        expect.objectContaining({ id: "rm_plan" })
      )
    })
  })

  it("closes a Session tab from the row context menu", async () => {
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )

    await waitFor(() => expect(h.listWorkbenchTabs).toHaveBeenCalledWith(2))
    fireEvent.contextMenu(
      await screen.findByRole("button", { name: "Evidence review" })
    )
    fireEvent.click(await screen.findByRole("menuitem", { name: "Close tab" }))

    expect(h.tabState.closeTab).toHaveBeenCalledWith("conversation:101")
  })

  it("moves a Session to another Workbench and drops the source tab", async () => {
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )

    await waitFor(() => expect(h.listWorkbenchTabs).toHaveBeenCalledWith(2))
    fireEvent.contextMenu(
      await screen.findByRole("button", { name: "Evidence review" })
    )
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Move to workbench" })
    )
    fireEvent.click(await screen.findByRole("menuitem", { name: "Review" }))

    await waitFor(() => {
      expect(h.appendConversationsToWorkbench).toHaveBeenCalledWith(
        2,
        [expect.objectContaining({ id: 101 })],
        "sidebar-bulk",
        { ignoreWorkbenchIds: [1] }
      )
      expect(h.tabState.closeTab).toHaveBeenCalledWith("conversation:101")
    })
  })

  it("opens a Session in a new Workbench without switching to it", async () => {
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )

    await waitFor(() => expect(h.listWorkbenchTabs).toHaveBeenCalledWith(2))
    fireEvent.contextMenu(
      await screen.findByRole("button", { name: "Evidence review" })
    )
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Open in new workbench" })
    )

    await waitFor(() => {
      expect(h.createOnly).toHaveBeenCalledWith("Workbench 3")
      expect(h.appendConversationsToWorkbench).toHaveBeenCalledWith(
        3,
        [expect.objectContaining({ id: 101 })],
        "sidebar-bulk",
        { ignoreWorkbenchIds: [1] }
      )
    })
    expect(h.tabState.closeTab).toHaveBeenCalledWith("conversation:101")
    expect(h.tabState.switchWorkbench).not.toHaveBeenCalled()
  })

  it("leaves Session actions off rows read from a saved snapshot", async () => {
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )

    await waitFor(() => expect(h.listWorkbenchTabs).toHaveBeenCalledWith(2))
    fireEvent.click(screen.getByRole("button", { name: "Expand workbench" }))
    fireEvent.contextMenu(
      await screen.findByRole("button", { name: "Reviewer session" })
    )

    const close = await screen.findByRole("menuitem", { name: "Close tab" })
    expect(close.getAttribute("data-disabled")).not.toBeNull()
    expect(
      screen
        .getByRole("menuitem", { name: "Move to workbench" })
        .getAttribute("data-disabled")
    ).not.toBeNull()
  })

  it("closes a Workbench view from its header context menu", async () => {
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )

    await waitFor(() => expect(h.listWorkbenchTabs).toHaveBeenCalledWith(2))
    fireEvent.contextMenu(screen.getByRole("button", { name: "Main" }))
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Close workbench tab" })
    )

    await waitFor(() => expect(h.closeView).toHaveBeenCalledWith(1))
  })

  it("renames a Workbench through the existing editor dialog", async () => {
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )

    await waitFor(() => expect(h.listWorkbenchTabs).toHaveBeenCalledWith(2))
    fireEvent.contextMenu(screen.getByRole("button", { name: "Review" }))
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Rename current workbench" })
    )
    fireEvent.click(await screen.findByRole("button", { name: "Save" }))

    await waitFor(() => expect(h.rename).toHaveBeenCalledWith(2, "Review"))
  })

  it("shows channel unread on a Room row", async () => {
    h.rooms.push({
      id: "rm_unread",
      workbenchId: 1,
      title: "Busy room",
      createdByConversationId: 101,
      memberCount: 2,
      unreadCount: 3,
      createdAt: "2026-08-06T00:00:00.000Z",
      updatedAt: "2026-08-06T00:00:00.000Z",
    })
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <WorkbenchTree />
      </NextIntlClientProvider>
    )
    const row = await screen.findByRole("button", { name: /Busy room/ })
    expect(row.textContent).toContain("3")
  })
})
