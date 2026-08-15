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
  duplicateAndSwitch: vi.fn(),
  rename: vi.fn(),
  setPinned: vi.fn(),
  remove: vi.fn(),
  switchWorkbench: vi.fn(),
  switchTab: vi.fn(),
  openTab: vi.fn(),
  openConversations: vi.fn(),
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
}))

vi.mock("@/stores/workbench-store", () => ({
  useWorkbenchStore: (selector: (state: unknown) => unknown) =>
    selector({
      items: h.workbenches,
      hydrated: true,
      loading: false,
      hydrate: h.hydrate,
      createAndSwitch: h.createAndSwitch,
      duplicateAndSwitch: h.duplicateAndSwitch,
      rename: h.rename,
      setPinned: h.setPinned,
      remove: h.remove,
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
})
