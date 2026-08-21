import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type {
  CollaborationRoomSummary,
  DbConversationSummary,
} from "@/lib/types"
import enMessages from "@/i18n/messages/en.json"

const h = vi.hoisted(() => ({
  archiveSessions: vi.fn(),
  deleteSessions: vi.fn(),
  moveSessionsToCollection: vi.fn(),
  appendConversationsToWorkbench: vi.fn(),
  appendRoomsToWorkbench: vi.fn(),
  closeConversationTab: vi.fn(),
  closeTab: vi.fn(),
  openTab: vi.fn(),
  openRoomTab: vi.fn(),
  openConversations: vi.fn(),
  createOnly: vi.fn(),
  createCollaborationRoom: vi.fn(),
  addCollaborationRoomMembers: vi.fn(),
  listConversationCollectionRefs: vi.fn(),
  roomCatalog: [] as CollaborationRoomSummary[],
  setRoute: vi.fn(),
  openRoom: vi.fn(),
  deleteRooms: vi.fn(),
  moveRoomsToCollection: vi.fn(),
  openRoomsInCurrentWorkbench: vi.fn(),
}))

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock("@/lib/session-bulk-operations", () => ({
  archiveSessions: h.archiveSessions,
  deleteSessions: h.deleteSessions,
  moveSessionsToCollection: h.moveSessionsToCollection,
}))

vi.mock("@/lib/room-bulk-operations", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/room-bulk-operations")>()
  return {
    ...actual,
    deleteRooms: h.deleteRooms,
    moveRoomsToCollection: h.moveRoomsToCollection,
    openRoomsInCurrentWorkbench: h.openRoomsInCurrentWorkbench,
  }
})

vi.mock("@/lib/api", () => ({
  createCollaborationRoom: h.createCollaborationRoom,
  addCollaborationRoomMembers: h.addCollaborationRoomMembers,
  listConversationCollectionRefs: h.listConversationCollectionRefs,
}))

// The real catalog store would hit the API on hydrate; the bar only reads the
// list, so a stub keeps "which Rooms are offered" under the test's control.
vi.mock("@/stores/room-catalog-store", () => ({
  useRoomCatalogStore: (selector: (state: unknown) => unknown) =>
    selector({ rooms: h.roomCatalog, hydrated: true, refresh: vi.fn() }),
}))

vi.mock("@/lib/open-room", () => ({
  useOpenRoom: () => h.openRoom,
  ROOM_TAB_PLACEHOLDER_AGENT: "claude_code",
  roomTabFolderId: () => 7,
}))

vi.mock("@/lib/workbench-session-tabs", () => ({
  appendConversationsToWorkbench: h.appendConversationsToWorkbench,
  appendRoomsToWorkbench: h.appendRoomsToWorkbench,
  conversationIdsOccupiedElsewhereFor: vi.fn(async () => new Set()),
  SESSION_CENTER_TAB_ORIGIN: "session-center",
  SIDEBAR_BULK_TAB_ORIGIN: "sidebar-bulk",
}))

vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => ({
    closeConversationTab: h.closeConversationTab,
    closeTab: h.closeTab,
    openTab: h.openTab,
    openRoomTab: h.openRoomTab,
  }),
}))

vi.mock("@/contexts/workbench-route-context", () => ({
  useWorkbenchRoute: () => ({
    openConversations: h.openConversations,
    setRoute: h.setRoute,
  }),
}))

vi.mock("@/stores/tab-store", () => ({
  useTabStore: (selector: (state: unknown) => unknown) =>
    selector({
      activeWorkbenchId: 1,
      rawTabs: [],
    }),
  makeRoomTabId: (roomId: string) => `room:${roomId}`,
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

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: unknown) => unknown) =>
    selector({
      folders: [{ id: 7 }],
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

function room(id: string): CollaborationRoomSummary {
  return {
    id,
    workbenchId: 1,
    title: `Room ${id}`,
    createdByConversationId: 1,
    memberCount: 2,
    unreadCount: 0,
    createdAt: "2026-06-01T00:00:00.000Z",
    updatedAt: "2026-06-01T00:00:00.000Z",
  }
}

function renderBar(
  selected = new Map([
    [1, conversation(1)],
    [2, conversation(2)],
  ]),
  onClear = vi.fn(),
  selectedRooms?: CollaborationRoomSummary[]
) {
  render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <SessionBulkActionBar
        selected={selected}
        selectedRooms={selectedRooms}
        onClear={onClear}
      />
    </NextIntlClientProvider>
  )
  return { onClear, user: userEvent.setup() }
}

/** "Create room" now lives at the bottom of the Join-room menu, next to the
 *  existing Rooms — same shape as the Workbench menu beside it. */
async function openJoinRoomMenu(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /Join room/ }))
}

describe("SessionBulkActionBar", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.roomCatalog = []
    h.archiveSessions.mockResolvedValue(undefined)
    h.deleteSessions.mockResolvedValue(undefined)
    h.moveSessionsToCollection.mockResolvedValue([])
    h.appendConversationsToWorkbench.mockResolvedValue({
      added: 2,
      skipped: 0,
      addedIds: [1, 2],
    })
    h.createCollaborationRoom.mockResolvedValue({
      id: "rm_test",
      title: "Session 1 / Session 2",
      workbenchId: 1,
      createdByConversationId: 1,
    })
    h.listConversationCollectionRefs.mockResolvedValue([])
    h.openRoom.mockResolvedValue(undefined)
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
      expect(h.appendConversationsToWorkbench).toHaveBeenCalledWith(
        2,
        [
          expect.objectContaining({ id: 1 }),
          expect.objectContaining({ id: 2 }),
        ],
        "sidebar-bulk",
        { ignoreWorkbenchIds: [1] }
      )
    )
    expect(h.openTab).not.toHaveBeenCalled()
  })

  it("creates a room from the current selection", async () => {
    const { onClear, user } = renderBar()
    await openJoinRoomMenu(user)
    await user.click(screen.getByRole("menuitem", { name: "New room" }))
    await waitFor(() =>
      expect(h.createCollaborationRoom).toHaveBeenCalledWith({
        workbenchId: 1,
        title: "Session 1 / Session 2",
        memberConversationIds: [1, 2],
        createdByConversationId: 1,
      })
    )
    expect(h.openRoom).toHaveBeenCalledWith(
      expect.objectContaining({ id: "rm_test" })
    )
    expect(onClear).toHaveBeenCalled()
  })

  it("places a new Room in the Collection shared by the selection", async () => {
    h.listConversationCollectionRefs.mockResolvedValue([
      { conversation_id: 1, collection_id: 10 },
      { conversation_id: 2, collection_id: 10 },
    ])
    const { user } = renderBar()
    await openJoinRoomMenu(user)
    await user.click(screen.getByRole("menuitem", { name: "New room" }))
    await waitFor(() =>
      expect(h.createCollaborationRoom).toHaveBeenCalledWith({
        workbenchId: 1,
        title: "Session 1 / Session 2",
        memberConversationIds: [1, 2],
        createdByConversationId: 1,
        collectionId: 10,
      })
    )
  })

  it("adds the selection to a Room that already exists", async () => {
    h.roomCatalog = [room("rm_a")]
    h.addCollaborationRoomMembers.mockResolvedValue({ id: "rm_a" })
    const { onClear, user } = renderBar()
    await openJoinRoomMenu(user)
    await user.click(screen.getByRole("menuitem", { name: /Room rm_a/ }))
    await waitFor(() =>
      expect(h.addCollaborationRoomMembers).toHaveBeenCalledWith({
        roomId: "rm_a",
        conversationIds: [1, 2],
      })
    )
    // Joining an existing Room must not create one.
    expect(h.createCollaborationRoom).not.toHaveBeenCalled()
    await waitFor(() => expect(onClear).toHaveBeenCalled())
  })

  it("asks for confirmation before deleting", async () => {
    const { user } = renderBar()
    await user.click(screen.getByRole("button", { name: "Delete" }))
    expect(h.deleteSessions).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Confirm" }))
    await waitFor(() => expect(h.deleteSessions).toHaveBeenCalled())
  })

  it("greys out Archive and Join room while a Room is in the selection", () => {
    renderBar(undefined, undefined, [room("rm_a")])
    expect(screen.getByRole("button", { name: "Archive" })).toBeDisabled()
    expect(screen.getByRole("button", { name: /Join room/ })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Delete" })).toBeEnabled()
  })

  it("counts Sessions and Rooms together in the selection label", () => {
    renderBar(undefined, undefined, [room("rm_a"), room("rm_b")])
    expect(screen.getByText("4 selected")).toBeInTheDocument()
  })

  it("moves a mixed selection, Rooms included", async () => {
    const { onClear, user } = renderBar(undefined, undefined, [room("rm_a")])
    await user.click(screen.getByRole("button", { name: /Move to collection/ }))
    await user.click(screen.getByRole("menuitem", { name: "Research" }))
    await waitFor(() => {
      expect(h.moveSessionsToCollection).toHaveBeenCalledWith([1, 2], 10)
      expect(h.moveRoomsToCollection).toHaveBeenCalledWith(
        [expect.objectContaining({ id: "rm_a" })],
        10
      )
    })
    expect(onClear).toHaveBeenCalled()
  })

  it("deletes a Rooms-only selection through the shared confirm dialog", async () => {
    h.deleteRooms.mockResolvedValue(undefined)
    const { user } = renderBar(new Map(), undefined, [room("rm_a")])
    await user.click(screen.getByRole("button", { name: "Delete" }))
    expect(screen.getByText("Delete 1 room(s)?")).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Confirm" }))
    await waitFor(() =>
      expect(h.deleteRooms).toHaveBeenCalledWith(
        [expect.objectContaining({ id: "rm_a" })],
        h.closeTab
      )
    )
    expect(h.deleteSessions).not.toHaveBeenCalled()
  })
})
