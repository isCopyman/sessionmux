import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { DbConversationSummary } from "@/lib/types"
import enMessages from "@/i18n/messages/en.json"

const h = vi.hoisted(() => ({
  archiveSessions: vi.fn(),
  deleteSessions: vi.fn(),
  moveSessionsToCollection: vi.fn(),
  appendConversationsToWorkbench: vi.fn(),
  closeConversationTab: vi.fn(),
  openTab: vi.fn(),
  openConversations: vi.fn(),
  createOnly: vi.fn(),
}))

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock("@/lib/session-bulk-operations", () => ({
  archiveSessions: h.archiveSessions,
  deleteSessions: h.deleteSessions,
  moveSessionsToCollection: h.moveSessionsToCollection,
}))

vi.mock("@/lib/workbench-session-tabs", () => ({
  appendConversationsToWorkbench: h.appendConversationsToWorkbench,
  SESSION_CENTER_TAB_ORIGIN: "session-center",
  SIDEBAR_BULK_TAB_ORIGIN: "sidebar-bulk",
}))

vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => ({
    closeConversationTab: h.closeConversationTab,
    openTab: h.openTab,
  }),
}))

vi.mock("@/contexts/workbench-route-context", () => ({
  useWorkbenchRoute: () => ({ openConversations: h.openConversations }),
}))

vi.mock("@/stores/tab-store", () => ({
  useTabStore: (selector: (state: unknown) => unknown) =>
    selector({
      activeWorkbenchId: 1,
      rawTabs: [],
    }),
}))

vi.mock("@/stores/workbench-store", () => ({
  useWorkbenchStore: (selector: (state: unknown) => unknown) =>
    selector({
      items: [
        { id: 1, name: "Main", position: 0, is_pinned: false },
        { id: 2, name: "Review", position: 1, is_pinned: false },
      ],
      hydrated: true,
      hydrate: vi.fn(),
      createOnly: h.createOnly,
    }),
}))

vi.mock("@/stores/collection-store", () => ({
  useCollectionStore: (selector: (state: unknown) => unknown) =>
    selector({
      items: [
        {
          id: 10,
          parent_id: null,
          name: "Research",
          position: 0,
          created_at: "2026-06-01T00:00:00.000Z",
          updated_at: "2026-06-01T00:00:00.000Z",
        },
      ],
      hydrated: true,
      hydrate: vi.fn(),
    }),
}))

import { SessionBulkActionBar } from "./session-bulk-action-bar"

function conversation(id: number): DbConversationSummary {
  return {
    id,
    folder_id: 7,
    title: `Session ${id}`,
    title_locked: true,
    agent_type: "codex",
    status: "in_progress",
    kind: "regular",
    model: null,
    git_branch: null,
    external_id: null,
    message_count: 1,
    child_count: 0,
    created_at: "2026-06-01T00:00:00.000Z",
    updated_at: "2026-06-01T00:00:00.000Z",
    archived_at: null,
    pinned_at: null,
  }
}

function renderBar(
  selected = new Map([
    [1, conversation(1)],
    [2, conversation(2)],
  ]),
  onClear = vi.fn()
) {
  render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <SessionBulkActionBar selected={selected} onClear={onClear} />
    </NextIntlClientProvider>
  )
  return { onClear, user: userEvent.setup() }
}

describe("SessionBulkActionBar", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.archiveSessions.mockResolvedValue(undefined)
    h.deleteSessions.mockResolvedValue(undefined)
    h.moveSessionsToCollection.mockResolvedValue([])
    h.appendConversationsToWorkbench.mockResolvedValue({
      added: 2,
      skipped: 0,
    })
  })

  it("archives the current selection", async () => {
    const { onClear, user } = renderBar()
    await user.click(screen.getByRole("button", { name: "Archive" }))
    await waitFor(() => expect(h.archiveSessions).toHaveBeenCalled())
    expect(onClear).toHaveBeenCalled()
  })

  it("moves the selection into a Collection", async () => {
    const { onClear, user } = renderBar()
    await user.click(screen.getByRole("button", { name: /Move to collection/ }))
    await user.click(screen.getByRole("menuitem", { name: "Research" }))
    await waitFor(() =>
      expect(h.moveSessionsToCollection).toHaveBeenCalledWith([1, 2], 10)
    )
    expect(onClear).toHaveBeenCalled()
  })

  it("adds the selection to another Workbench without opening it", async () => {
    const { user } = renderBar()
    await user.click(screen.getByRole("button", { name: /Add to workbench/ }))
    await user.click(screen.getByRole("menuitem", { name: "Review" }))
    await waitFor(() =>
      expect(h.appendConversationsToWorkbench).toHaveBeenCalledWith(2, [
        expect.objectContaining({ id: 1 }),
        expect.objectContaining({ id: 2 }),
      ])
    )
    expect(h.openTab).not.toHaveBeenCalled()
  })

  it("asks for confirmation before deleting", async () => {
    const { user } = renderBar()
    await user.click(screen.getByRole("button", { name: "Delete" }))
    expect(h.deleteSessions).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Confirm" }))
    await waitFor(() => expect(h.deleteSessions).toHaveBeenCalled())
  })
})
