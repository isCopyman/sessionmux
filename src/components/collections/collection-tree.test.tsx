import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { createRef, type RefObject } from "react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import {
  CollectionTree,
  nearestRootFolderId,
  type CollectionTreeHandle,
} from "./collection-tree"
import {
  canDropSessionOnTarget,
  collectionPlacementForRoot,
  collectionPlacementForRow,
  sessionDragPayload,
  sessionIdsInDrag,
} from "./collection-tree-dnd"
import enMessages from "@/i18n/messages/en.json"
import type { CollectionInfo, DbConversationSummary } from "@/lib/types"

const h = vi.hoisted(() => ({
  create: vi.fn(),
  rename: vi.fn(),
  move: vi.fn(),
  place: vi.fn(),
  remove: vi.fn(),
  hydrate: vi.fn(),
  listRefs: vi.fn(),
  assignCollection: vi.fn(),
  assignRooms: vi.fn(),
  updateTitle: vi.fn(),
  updateArchive: vi.fn(),
  updatePinned: vi.fn(),
  updateStatus: vi.fn(),
  deleteConversation: vi.fn(),
  updateConversationLocal: vi.fn(),
  applyConversationUpsert: vi.fn(),
  applyConversationRemove: vi.fn(),
  closeConversationTab: vi.fn(),
  closeTab: vi.fn(),
  openTab: vi.fn(),
  switchTab: vi.fn(),
  openConversations: vi.fn(),
  openRoom: vi.fn(),
  openForFolder: vi.fn(),
  deleteRoom: vi.fn(),
  activeTab: {
    id: "conv-102",
    kind: "conversation" as "conversation" | "room",
    conversationId: 102 as number | null,
    roomId: undefined as string | undefined,
  },
  draftTabs: [] as Array<{
    id: string
    kind: "conversation"
    folderId: number
    conversationId: null
    agentType: string
    title: string
  }>,
  rooms: [] as Array<{
    id: string
    workbenchId: number
    title: string
    createdByConversationId: number
    collectionId?: number | null
    rootFolderId?: number | null
    memberCount: number
    unreadCount: number
    updatedAt: string
    createdAt: string
    lastEventAt?: string | null
  }>,
  // One Session per source, plus 101 which deliberately records none — that is
  // what a row written before the `created_by` column looks like, and it must
  // read as user-created rather than vanish behind the source facet.
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
      message_count: 3,
      child_count: 0,
      created_at: "2026-06-03T00:00:00.000Z",
      updated_at: "2026-06-03T00:00:00.000Z",
      archived_at: null,
      pinned_at: null,
    },
    {
      id: 102,
      folder_id: 7,
      title: "Loose notes",
      title_locked: true,
      agent_type: "claude",
      status: "in_progress",
      kind: "regular",
      model: null,
      git_branch: null,
      external_id: "session-102",
      message_count: 2,
      child_count: 0,
      created_at: "2026-06-02T00:00:00.000Z",
      updated_at: "2026-06-02T00:00:00.000Z",
      archived_at: null,
      pinned_at: null,
      created_by: "user",
    },
    {
      id: 103,
      folder_id: 8,
      title: "Worktree experiment",
      title_locked: true,
      agent_type: "codex",
      status: "in_progress",
      kind: "regular",
      model: null,
      git_branch: "experiment",
      external_id: "session-103",
      message_count: 1,
      child_count: 0,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
      archived_at: null,
      pinned_at: null,
      created_by: "agent",
    },
    {
      id: 104,
      folder_id: 9,
      title: "Other project notes",
      title_locked: true,
      agent_type: "gemini",
      status: "in_progress",
      kind: "regular",
      model: null,
      git_branch: null,
      external_id: "session-104",
      message_count: 1,
      child_count: 0,
      created_at: "2026-05-31T00:00:00.000Z",
      updated_at: "2026-05-31T00:00:00.000Z",
      archived_at: null,
      pinned_at: null,
      created_by: "automation",
    },
  ],
  items: [
    {
      id: 10,
      // Widened so tests can push legacy Collections with a null root.
      root_folder_id: 7 as number | null,
      parent_id: null,
      name: "Research",
      position: 0,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    },
    {
      id: 11,
      root_folder_id: 7,
      parent_id: 10,
      name: "Sources",
      position: 0,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    },
    {
      id: 12,
      root_folder_id: 9,
      parent_id: null,
      name: "Other research",
      position: 0,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    },
    {
      id: 13,
      root_folder_id: 7,
      parent_id: null,
      name: "Writing",
      position: 1,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    },
  ],
}))

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock("@/stores/collection-store", () => ({
  useCollectionStore: (selector: (state: unknown) => unknown) =>
    selector({
      items: h.items,
      hydrated: true,
      loading: false,
      hydrate: h.hydrate,
      create: h.create,
      rename: h.rename,
      move: h.move,
      place: h.place,
      remove: h.remove,
    }),
}))

vi.mock("@/stores/app-workspace-store", () => {
  const folders = [
    {
      id: 7,
      name: "project",
      alias: null,
      path: "/tmp/project",
      parent_id: null,
      kind: "regular",
    },
    {
      id: 8,
      name: "experiment",
      alias: null,
      path: "/tmp/project-worktrees/experiment",
      parent_id: 7,
      kind: "worktree",
    },
    {
      id: 9,
      name: "other-project",
      alias: null,
      path: "/tmp/other-project",
      parent_id: null,
      kind: "regular",
    },
  ]
  const state = {
    conversations: h.conversations,
    activeFolderId: 7,
    folders,
    updateConversationLocal: h.updateConversationLocal,
    applyConversationUpsert: h.applyConversationUpsert,
    applyConversationRemove: h.applyConversationRemove,
    allFolders: folders,
  }
  const useAppWorkspaceStore = (selector: (value: typeof state) => unknown) =>
    selector(state)
  useAppWorkspaceStore.getState = () => state
  return { useAppWorkspaceStore }
})

vi.mock("@/contexts/tab-context", () => ({
  useTabStore: (selector: (state: unknown) => unknown) =>
    selector({
      activeTabId: h.activeTab.id,
      tabs: [
        {
          id: h.activeTab.id,
          kind: h.activeTab.kind,
          conversationId: h.activeTab.conversationId,
          roomId: h.activeTab.roomId,
        },
        ...h.draftTabs,
      ],
    }),
  useTabActions: () => ({
    closeConversationTab: h.closeConversationTab,
    closeTab: h.closeTab,
    openTab: h.openTab,
    switchTab: h.switchTab,
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
  makeRoomTabId: (roomId: string) => `room-${roomId}`,
}))

vi.mock("@/stores/workbench-store", () => ({
  useWorkbenchStore: (selector: (state: unknown) => unknown) =>
    selector({
      items: [{ id: 1, name: "Main", position: 0, is_pinned: false }],
      hydrated: true,
      hydrate: vi.fn(),
      createOnly: vi.fn(),
    }),
}))

vi.mock("@/components/conversations/session-details-dialog", () => ({
  SessionDetailsDialog: () => null,
}))

vi.mock("@/lib/open-room", () => ({
  useOpenRoom: () => h.openRoom,
  ROOM_TAB_PLACEHOLDER_AGENT: "claude_code",
  roomTabFolderId: () => 7,
}))

vi.mock("@/contexts/create-room-dialog-context", () => ({
  useCreateRoomDialog: () => ({
    open: false,
    setOpen: vi.fn(),
    folderScopeId: null,
    openForFolder: h.openForFolder,
  }),
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

vi.mock("@/lib/api", () => ({
  listConversationCollectionRefs: h.listRefs,
  assignConversationsToCollection: h.assignCollection,
  assignRoomsToCollection: h.assignRooms,
  updateConversationTitle: h.updateTitle,
  updateConversationArchive: h.updateArchive,
  updateConversationPinned: h.updatePinned,
  updateConversationStatus: h.updateStatus,
  deleteConversation: h.deleteConversation,
  deleteCollaborationRoom: h.deleteRoom,
}))

const rect = (top: number, height = 28): DOMRect =>
  ({
    x: 0,
    y: top,
    top,
    left: 0,
    right: 220,
    bottom: top + height,
    width: 220,
    height,
    toJSON: () => ({}),
  }) as DOMRect

function renderTree(
  onOpenScope = vi.fn(),
  options: {
    showSessions?: boolean
    showAgentCreated?: boolean
    showAutomationCreated?: boolean
    onOpenSession?: (session: DbConversationSummary) => void
    onOpenSessionInSplit?: (
      session: DbConversationSummary,
      direction: "right" | "down"
    ) => void
    onNewSession?: (rootFolderId: number) => void
    onNewSessionInCollection?: (collectionId: number) => void
    treeRef?: RefObject<CollectionTreeHandle | null>
  } = {}
) {
  const { treeRef, ...treeOptions } = options
  const buildElement = () => (
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <CollectionTree
        ref={treeRef}
        onOpenScope={onOpenScope}
        {...treeOptions}
      />
    </NextIntlClientProvider>
  )
  const utils = render(buildElement())
  return {
    user: userEvent.setup(),
    onOpenScope,
    // Fresh element each time: re-rendering the same element reference lets
    // React bail out of rendering entirely, and the mocked stores (read
    // during render) would never see the updated hoisted fixtures.
    rerenderTree: () => utils.rerender(buildElement()),
  }
}

describe("CollectionTree", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.rooms.length = 0
    h.draftTabs.length = 0
    h.activeTab.id = "conv-102"
    h.activeTab.kind = "conversation"
    h.activeTab.conversationId = 102
    h.activeTab.roomId = undefined
    h.deleteRoom.mockResolvedValue(undefined)
    h.create.mockResolvedValue({
      id: 14,
      root_folder_id: 7,
      parent_id: null,
      name: "Writing",
      position: 1,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    })
    h.listRefs.mockResolvedValue([{ conversation_id: 101, collection_id: 11 }])
    h.assignCollection.mockImplementation(
      async (conversationIds: number[], collectionId: number | null) =>
        collectionId == null
          ? []
          : conversationIds.map((conversationId) => ({
              conversation_id: conversationId,
              collection_id: collectionId,
            }))
    )
    h.assignRooms.mockResolvedValue([])
    h.place.mockResolvedValue(undefined)
  })

  it("opens nested Collections as Session Center scopes", async () => {
    const { user, onOpenScope } = renderTree()
    await user.click(
      screen.getAllByRole("button", { name: "Expand collection" })[0]
    )
    await user.click(screen.getByTitle("Sources"))

    expect(onOpenScope).toHaveBeenCalledWith(11)
  })

  it("expands and scrolls to the active Room", async () => {
    const scrollIntoView = vi.fn()
    HTMLElement.prototype.scrollIntoView = scrollIntoView
    h.rooms.push({
      id: "rm_notes",
      workbenchId: 1,
      title: "Notes room",
      createdByConversationId: 102,
      collectionId: null,
      rootFolderId: 7,
      memberCount: 2,
      unreadCount: 0,
      createdAt: "2026-06-04T00:00:00.000Z",
      updatedAt: "2026-06-04T00:00:00.000Z",
    })
    h.activeTab.id = "room-rm_notes"
    h.activeTab.kind = "room"
    h.activeTab.conversationId = null
    h.activeTab.roomId = "rm_notes"
    const treeRef = createRef<CollectionTreeHandle>()
    const { user } = renderTree(vi.fn(), { showSessions: true, treeRef })

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Notes room" })).toBeTruthy()
    )
    await user.click(screen.getByText("project").closest("button")!)
    expect(screen.queryByRole("button", { name: "Notes room" })).toBeNull()

    act(() => treeRef.current?.scrollToActive())

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Notes room" })).toBeTruthy()
    )
    expect(scrollIntoView).toHaveBeenCalledWith({
      behavior: "smooth",
      block: "center",
    })
  })

  it("expands and scrolls to the active Session", async () => {
    const scrollIntoView = vi.fn()
    HTMLElement.prototype.scrollIntoView = scrollIntoView
    const treeRef = createRef<CollectionTreeHandle>()
    const { user } = renderTree(vi.fn(), { showSessions: true, treeRef })

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Loose notes" })).toBeTruthy()
    )
    await user.click(screen.getByText("project").closest("button")!)
    expect(screen.queryByRole("button", { name: "Loose notes" })).toBeNull()

    act(() => treeRef.current?.scrollToActive())

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Loose notes" })).toBeTruthy()
    )
    expect(scrollIntoView).toHaveBeenCalledWith({
      behavior: "smooth",
      block: "center",
    })
  })

  it("creates a top-level Collection without touching execution folders", async () => {
    const { user, onOpenScope } = renderTree()
    await user.click(screen.getByRole("button", { name: "New collection" }))
    expect(screen.queryByRole("combobox")).toBeNull()
    await user.type(screen.getByPlaceholderText("Collection name"), "Writing")
    await user.click(screen.getByRole("button", { name: "Confirm" }))

    expect(h.create).toHaveBeenCalledWith("Writing", null, 7)
    expect(onOpenScope).toHaveBeenCalledWith(14)
  })

  it("creates a nested Collection at the invoked level without a parent picker", async () => {
    renderTree()
    fireEvent.contextMenu(screen.getByRole("button", { name: "Research" }))
    await userEvent.click(
      screen.getByRole("menuitem", { name: "New nested collection" })
    )

    expect(screen.queryByRole("combobox")).toBeNull()
    await userEvent.type(
      screen.getByPlaceholderText("Collection name"),
      "Subtopic"
    )
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }))

    expect(h.create).toHaveBeenCalledWith("Subtopic", 10, 7)
  })

  it("expands Collections into inline Sessions and keeps Unclassified visible", async () => {
    const onOpenSession = vi.fn()
    const onNewSession = vi.fn()
    const { user } = renderTree(vi.fn(), {
      showSessions: true,
      onOpenSession,
      onNewSession,
    })

    expect(await screen.findByText("project")).toBeTruthy()
    expect(await screen.findByText("Loose notes")).toBeTruthy()
    expect(await screen.findByText("Worktree experiment")).toBeTruthy()
    expect(
      document.querySelector('[data-focused-session="true"]')?.textContent
    ).toContain("Loose notes")
    expect(document.querySelectorAll("[data-collection-path]")).toHaveLength(2)
    await user.click(screen.getByRole("button", { name: "New chat · project" }))
    expect(onNewSession).toHaveBeenCalledWith(7)
    await user.click(screen.getByRole("button", { name: "Research" }))
    await user.click(screen.getByTitle("Sources"))
    await user.click(await screen.findByText("Evidence review"))

    expect(onOpenSession).toHaveBeenCalledWith(
      expect.objectContaining({ id: 101, title: "Evidence review" })
    )
  })

  it("shows an open draft tab under the Collection owning its Path", async () => {
    h.draftTabs.push(
      {
        id: "draft-1",
        kind: "conversation",
        folderId: 7,
        conversationId: null,
        agentType: "claude",
        title: "",
      },
      {
        id: "draft-2",
        kind: "conversation",
        folderId: 8,
        conversationId: null,
        agentType: "codex",
        title: "Worktree draft",
      },
      {
        id: "draft-chat",
        kind: "conversation",
        folderId: 0,
        conversationId: null,
        agentType: "claude",
        title: "",
      }
    )
    const { user } = renderTree(vi.fn(), { showSessions: true })

    await user.click(screen.getByRole("button", { name: "Research" }))

    // An untitled draft falls back to the Workbench draft label; the worktree
    // draft resolves through the worktree's parent Path to the same root.
    expect(await screen.findByText("New session")).toBeTruthy()
    expect(screen.getByText("Worktree draft")).toBeTruthy()
    // The chat-mode draft (folderId 0) owns no Path and never renders here.
    expect(document.querySelectorAll("[data-draft-tab-id]")).toHaveLength(2)
    expect(
      document.querySelector('[data-draft-tab-id="draft-chat"]')
    ).toBeNull()
    // Drafts read as placeholders next to the DB-backed rows: italic + muted.
    const draftRow = document.querySelector('[data-draft-tab-id="draft-1"]')
    expect(draftRow?.querySelector(".italic")).toBeTruthy()
  })

  it("activates the draft tab on click instead of opening a conversation", async () => {
    const onOpenSession = vi.fn()
    h.draftTabs.push({
      id: "draft-1",
      kind: "conversation",
      folderId: 7,
      conversationId: null,
      agentType: "claude",
      title: "",
    })
    const { user } = renderTree(vi.fn(), { showSessions: true, onOpenSession })
    await user.click(screen.getByRole("button", { name: "Research" }))

    await user.click(await screen.findByRole("button", { name: "New session" }))

    expect(h.switchTab).toHaveBeenCalledWith("draft-1")
    expect(h.openConversations).toHaveBeenCalled()
    expect(h.openTab).not.toHaveBeenCalled()
    expect(onOpenSession).not.toHaveBeenCalled()
  })

  it("drops the draft row once its tab is gone", async () => {
    h.draftTabs.push({
      id: "draft-1",
      kind: "conversation",
      folderId: 7,
      conversationId: null,
      agentType: "claude",
      title: "",
    })
    const { user, rerenderTree } = renderTree(vi.fn(), { showSessions: true })
    await user.click(screen.getByRole("button", { name: "Research" }))
    expect(await screen.findByText("New session")).toBeTruthy()

    h.draftTabs.length = 0
    rerenderTree()

    expect(screen.queryByText("New session")).toBeNull()
    expect(document.querySelector("[data-draft-tab-id]")).toBeNull()
  })

  it("offers New Conversation on a Collection's context menu", async () => {
    const onNewSessionInCollection = vi.fn()
    renderTree(vi.fn(), { onNewSessionInCollection })

    fireEvent.contextMenu(screen.getByRole("button", { name: "Research" }))
    await userEvent.click(
      screen.getByRole("menuitem", { name: "New Conversation" })
    )

    expect(onNewSessionInCollection).toHaveBeenCalledWith(10)
  })

  it("offers New Conversation on a Collection's hover menu", async () => {
    const onNewSessionInCollection = vi.fn()
    const { user } = renderTree(vi.fn(), { onNewSessionInCollection })

    await user.click(
      screen.getByRole("button", { name: "Actions for Research" })
    )
    await user.click(screen.getByRole("menuitem", { name: "New Conversation" }))

    expect(onNewSessionInCollection).toHaveBeenCalledWith(10)
  })

  it("reports the nested Collection's own id, not its ancestor's", async () => {
    const onNewSessionInCollection = vi.fn()
    const { user } = renderTree(vi.fn(), { onNewSessionInCollection })

    await user.click(
      screen.getAllByRole("button", { name: "Expand collection" })[0]
    )
    fireEvent.contextMenu(screen.getByRole("button", { name: "Sources" }))
    await userEvent.click(
      screen.getByRole("menuitem", { name: "New Conversation" })
    )

    expect(onNewSessionInCollection).toHaveBeenCalledWith(11)
  })

  it("offers New room on a Collection context menu with that Path as scope", async () => {
    const onNewSessionInCollection = vi.fn()
    renderTree(vi.fn(), { onNewSessionInCollection })

    fireEvent.contextMenu(screen.getByRole("button", { name: "Research" }))
    const newSession = screen.getByRole("menuitem", {
      name: "New Conversation",
    })
    const newRoom = screen.getByRole("menuitem", { name: "New room" })
    expect(
      newSession.compareDocumentPosition(newRoom) &
        Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()

    await userEvent.click(newRoom)

    expect(h.openForFolder).toHaveBeenCalledWith(7)
    expect(onNewSessionInCollection).not.toHaveBeenCalled()
  })

  it("offers New room on a Collection hover menu with that Path as scope", async () => {
    const { user } = renderTree(vi.fn(), {
      onNewSessionInCollection: vi.fn(),
    })

    await user.click(
      screen.getByRole("button", { name: "Actions for Research" })
    )
    await user.click(screen.getByRole("menuitem", { name: "New room" }))

    expect(h.openForFolder).toHaveBeenCalledWith(7)
  })

  it("offers New room on a Path folder context menu with that folder id", async () => {
    renderTree(vi.fn(), { showSessions: true, onNewSession: vi.fn() })

    const pathName = await screen.findByText("project")
    fireEvent.contextMenu(pathName)
    const newSession = screen.getByRole("menuitem", {
      name: "New Conversation",
    })
    const newRoom = screen.getByRole("menuitem", { name: "New room" })
    expect(
      newSession.compareDocumentPosition(newRoom) &
        Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()

    await userEvent.click(newRoom)

    expect(h.openForFolder).toHaveBeenCalledWith(7)
  })

  it("hides New Conversation on a legacy Collection with no canonical Path", async () => {
    h.items.push({
      id: 20,
      root_folder_id: null,
      parent_id: null,
      name: "Legacy inbox",
      position: 9,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    })
    try {
      renderTree(vi.fn(), { onNewSessionInCollection: vi.fn() })

      fireEvent.contextMenu(
        screen.getByRole("button", { name: "Legacy inbox" })
      )

      expect(
        screen.queryByRole("menuitem", { name: "New Conversation" })
      ).toBeNull()
      expect(screen.queryByRole("menuitem", { name: "New room" })).toBeNull()
      // The rest of the menu is untouched.
      expect(
        screen.getByRole("menuitem", { name: "New nested collection" })
      ).toBeTruthy()
    } finally {
      h.items.length = 4
    }
  })

  it("drops agent-created Sessions when the source switch is off", async () => {
    renderTree(vi.fn(), { showSessions: true, showAgentCreated: false })

    // 102 is user-created and 103 was started by a delegating agent.
    expect(await screen.findByText("Loose notes")).toBeTruthy()
    expect(screen.queryByText("Worktree experiment")).toBeNull()
  })

  it("keeps a Session with no recorded source under both switches off", async () => {
    const { user } = renderTree(vi.fn(), {
      showSessions: true,
      showAgentCreated: false,
      showAutomationCreated: false,
    })
    // Wait for the tree to hydrate before reaching for a Collection row.
    expect(await screen.findByText("Loose notes")).toBeTruthy()

    // 101 predates the column: "hide what agents and automations started" must
    // never be able to hide it, so the facet can't empty the tree.
    await user.click(screen.getByRole("button", { name: "Research" }))
    await user.click(screen.getByTitle("Sources"))
    expect(await screen.findByText("Evidence review")).toBeTruthy()
  })

  it("opens an inline Collection Session directly in a chosen pane", async () => {
    const onOpenSessionInSplit = vi.fn()
    renderTree(vi.fn(), {
      showSessions: true,
      onOpenSessionInSplit,
    })

    const looseNotes = await screen.findByText("Loose notes")
    fireEvent.contextMenu(looseNotes)
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Open to the right" })
    )

    expect(onOpenSessionInSplit).toHaveBeenCalledWith(
      expect.objectContaining({ id: 102, title: "Loose notes" }),
      "right"
    )
  })

  // ctrl/shift+click was the only way in, so the bulk bar below the tree was
  // unreachable for anyone who did not already know the shortcut.
  it("starts a multi-selection from the Session context menu", async () => {
    renderTree(vi.fn(), { showSessions: true })

    const looseNotes = await screen.findByText("Loose notes")
    fireEvent.contextMenu(looseNotes)
    const select = screen.getByRole("menuitem", { name: /Select/ })
    // The item teaches the shortcut it replaces.
    expect(select.textContent).toContain("Click")
    await userEvent.click(select)

    // One selected Session means the bulk bar is now on screen.
    expect(await screen.findByText("1 selected")).toBeTruthy()
  })

  it("accepts a Session Collection change without changing its Path", () => {
    expect(
      canDropSessionOnTarget(
        {
          kind: "session",
          conversationId: 102,
          rootFolderId: 7,
          label: "Loose notes",
        },
        7,
        10,
        null
      )
    ).toBe(true)
  })

  it("accepts moving a classified Session back to Unclassified", () => {
    expect(
      canDropSessionOnTarget(
        {
          kind: "session",
          conversationId: 101,
          rootFolderId: 7,
          label: "Evidence review",
        },
        7,
        null,
        11
      )
    ).toBe(true)
  })

  it("does not move a Session to its current Collection", () => {
    expect(
      canDropSessionOnTarget(
        {
          kind: "session",
          conversationId: 101,
          rootFolderId: 7,
          label: "Evidence review",
        },
        7,
        11,
        11
      )
    ).toBe(false)
  })

  it("drags every selected Session on the same Path together", () => {
    const payload = sessionDragPayload({
      grabbedId: 102,
      grabbedRootFolderId: 7,
      grabbedLabel: "Loose notes",
      selectedIds: [102, 103, 104],
      rootFolderIdByConversation: new Map([
        [102, 7],
        [103, 7],
        [104, 9],
      ]),
    })
    expect(sessionIdsInDrag(payload)).toEqual([102, 103])
    expect(canDropSessionOnTarget(payload, 7, 10, null, false, new Map())).toBe(
      true
    )
  })

  it("keeps a plain drag on an unselected Session as a single move", () => {
    const payload = sessionDragPayload({
      grabbedId: 102,
      grabbedRootFolderId: 7,
      grabbedLabel: "Loose notes",
      selectedIds: [101, 103],
      rootFolderIdByConversation: new Map([
        [101, 7],
        [102, 7],
        [103, 7],
      ]),
    })
    expect(sessionIdsInDrag(payload)).toEqual([102])
  })

  it("rejects Session drops across canonical Paths", () => {
    expect(
      canDropSessionOnTarget(
        {
          kind: "session",
          conversationId: 102,
          rootFolderId: 7,
          label: "Loose notes",
        },
        9,
        12,
        null
      )
    ).toBe(false)
  })

  it("resolves the top quarter of a Collection row as before", () => {
    expect(
      collectionPlacementForRow(
        {
          kind: "collection",
          collectionId: 13,
          rootFolderId: 7,
          label: "Writing",
        },
        h.items[0],
        rect(100),
        102,
        h.items
      )
    ).toMatchObject({ parentId: null, index: 0, position: "before" })
  })

  it("resolves the center of a Collection row as nesting", () => {
    expect(
      collectionPlacementForRow(
        {
          kind: "collection",
          collectionId: 13,
          rootFolderId: 7,
          label: "Writing",
        },
        h.items[0],
        rect(100),
        114,
        h.items
      )
    ).toMatchObject({ parentId: 10, index: 1, position: "inside" })
  })

  it("promotes a nested Collection when dropped on its Path header", () => {
    expect(
      collectionPlacementForRoot(
        {
          kind: "collection",
          collectionId: 11,
          rootFolderId: 7,
          label: "Sources",
        },
        7,
        h.items
      )
    ).toMatchObject({ parentId: null, index: 2, position: "root" })
  })

  it("does not persist a Collection when it is dropped back in place", () => {
    expect(
      collectionPlacementForRow(
        {
          kind: "collection",
          collectionId: 10,
          rootFolderId: 7,
          label: "Research",
        },
        h.items[3],
        rect(100),
        102,
        h.items
      )
    ).toBeNull()
  })

  it("rejects Collection cycles and cross-Path placement", () => {
    const research = {
      kind: "collection" as const,
      collectionId: 10,
      rootFolderId: 7,
      label: "Research",
    }
    expect(
      collectionPlacementForRow(research, h.items[1], rect(100), 114, h.items)
    ).toBeNull()
    expect(
      collectionPlacementForRow(research, h.items[2], rect(100), 114, h.items)
    ).toBeNull()
  })

  it("renames a Session from the tree context menu", async () => {
    h.updateTitle.mockResolvedValue(undefined)
    renderTree(vi.fn(), { showSessions: true })
    fireEvent.contextMenu(await screen.findByText("Loose notes"))
    await userEvent.click(screen.getByRole("menuitem", { name: /Rename/ }))
    const input = screen.getByRole("textbox")
    await userEvent.clear(input)
    await userEvent.type(input, "Session notes")
    await userEvent.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() =>
      expect(h.updateTitle).toHaveBeenCalledWith(102, "Session notes")
    )
  })

  it("archives a Session from the tree context menu", async () => {
    h.updateArchive.mockResolvedValue(undefined)
    renderTree(vi.fn(), { showSessions: true })
    fireEvent.contextMenu(await screen.findByText("Loose notes"))
    await userEvent.click(screen.getByRole("menuitem", { name: "Archive" }))
    await waitFor(() => expect(h.updateArchive).toHaveBeenCalledWith(102, true))
    expect(h.applyConversationUpsert).toHaveBeenCalledWith(
      expect.objectContaining({ id: 102, archived_at: expect.any(String) })
    )
  })

  it("renames a Collection with F2", async () => {
    renderTree(vi.fn(), { showSessions: true })
    const research = screen.getByRole("button", { name: "Research" })
    research.focus()
    fireEvent.keyDown(research, { key: "F2" })
    expect(await screen.findByText("Rename collection")).toBeTruthy()
  })

  it("selects multiple Sessions with modifier clicks and archives them", async () => {
    const onOpenSession = vi.fn()
    renderTree(vi.fn(), { showSessions: true, onOpenSession })
    const looseNotes = await screen.findByRole("button", {
      name: "Loose notes",
    })
    fireEvent.click(looseNotes, { ctrlKey: true })
    fireEvent.click(
      screen.getByRole("button", { name: "Worktree experiment" }),
      { ctrlKey: true }
    )

    expect(onOpenSession).not.toHaveBeenCalled()
    expect(await screen.findByText("2 selected")).toBeTruthy()
    const checkedRow = document.querySelector('[data-session-checked="true"]')
    expect(checkedRow?.querySelector("[data-session-agent-icon]")).toBeTruthy()
    expect(
      checkedRow
        ?.querySelector("[data-session-agent-icon]")
        ?.classList.contains("opacity-0")
    ).toBe(false)

    await userEvent.click(screen.getByRole("button", { name: "Archive" }))
    await waitFor(() => {
      expect(h.updateArchive).toHaveBeenCalledWith(102, true)
      expect(h.updateArchive).toHaveBeenCalledWith(103, true)
    })
    expect(screen.queryByText("2 selected")).toBeNull()
  })

  it("marks same-Path selected Sessions as one drag group", async () => {
    renderTree(vi.fn(), { showSessions: true })
    fireEvent.click(
      await screen.findByRole("button", { name: "Loose notes" }),
      {
        ctrlKey: true,
      }
    )
    fireEvent.click(
      screen.getByRole("button", { name: "Worktree experiment" }),
      { ctrlKey: true }
    )
    expect(
      document
        .querySelector('[data-conversation-id="102"]')
        ?.getAttribute("data-session-drag-ids")
    ).toBe("102,103")
    expect(
      document
        .querySelector('[data-conversation-id="104"]')
        ?.getAttribute("data-session-drag-ids")
    ).toBe("104")
  })

  it("requires confirmation before deleting the selected Sessions", async () => {
    h.deleteConversation.mockResolvedValue(undefined)
    renderTree(vi.fn(), { showSessions: true })
    fireEvent.click(
      await screen.findByRole("button", { name: "Loose notes" }),
      {
        ctrlKey: true,
      }
    )
    fireEvent.click(
      screen.getByRole("button", { name: "Worktree experiment" }),
      { ctrlKey: true }
    )

    await userEvent.click(screen.getByRole("button", { name: "Delete" }))
    expect(h.deleteConversation).not.toHaveBeenCalled()
    expect(await screen.findByText("Delete 2 conversation(s)?")).toBeTruthy()

    await userEvent.click(screen.getByRole("button", { name: "Confirm" }))
    await waitFor(() => {
      expect(h.deleteConversation).toHaveBeenCalledWith(102)
      expect(h.deleteConversation).toHaveBeenCalledWith(103)
    })
  })

  it("shows a Room on its own Path, not under the creator Session", async () => {
    h.rooms.push({
      id: "rm_notes",
      workbenchId: 1,
      title: "Notes room",
      createdByConversationId: 102,
      collectionId: null,
      rootFolderId: 7,
      memberCount: 2,
      unreadCount: 0,
      createdAt: "2026-06-04T00:00:00.000Z",
      updatedAt: "2026-06-04T00:00:00.000Z",
    })
    renderTree(vi.fn(), { showSessions: true })
    expect(await screen.findByText("Notes room")).toBeTruthy()
    expect(await screen.findByText("Loose notes")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Notes room" }))
    await waitFor(() => {
      expect(h.openRoom).toHaveBeenCalledWith(
        expect.objectContaining({ id: "rm_notes" })
      )
    })
  })

  it("shows channel unread on a Room row", async () => {
    h.rooms.push({
      id: "rm_unread",
      workbenchId: 1,
      title: "Busy room",
      createdByConversationId: 102,
      collectionId: null,
      rootFolderId: 7,
      memberCount: 2,
      unreadCount: 3,
      createdAt: "2026-06-04T00:00:00.000Z",
      updatedAt: "2026-06-04T00:00:00.000Z",
    })
    renderTree(vi.fn(), { showSessions: true })
    // Anchored: the multi-select checkbox is named "Select Busy room", which a
    // bare /Busy room/ would also match.
    const row = await screen.findByRole("button", { name: /^Busy room/ })
    expect(row.textContent).toContain("3")
  })

  it("shows a Room under the Collection it owns, not the creator Session", async () => {
    h.rooms.push({
      id: "rm_sources",
      workbenchId: 1,
      title: "Sources room",
      createdByConversationId: 102,
      collectionId: 11,
      rootFolderId: 7,
      memberCount: 2,
      unreadCount: 0,
      createdAt: "2026-06-04T00:00:00.000Z",
      updatedAt: "2026-06-04T00:00:00.000Z",
    })
    const { user } = renderTree(vi.fn(), { showSessions: true })
    await user.click(screen.getByRole("button", { name: "Research" }))
    await user.click(screen.getByTitle("Sources"))
    expect(await screen.findByText("Sources room")).toBeTruthy()
    expect(screen.queryByText("Loose notes")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Sources room" }))
    await waitFor(() => {
      expect(h.openRoom).toHaveBeenCalledWith(
        expect.objectContaining({ id: "rm_sources", collectionId: 11 })
      )
    })
  })

  it("keeps a Room with no Collection and no Path reachable in the tree", async () => {
    // What `room.create` over MCP used to persist: no Collection, no Path, so
    // neither the Collection buckets nor any Path's Unclassified group held it.
    h.rooms.push({
      id: "rm_orphan",
      workbenchId: 1,
      title: "Formation room",
      createdByConversationId: 102,
      collectionId: null,
      rootFolderId: null,
      memberCount: 2,
      unreadCount: 0,
      createdAt: "2026-06-04T00:00:00.000Z",
      updatedAt: "2026-06-04T00:00:00.000Z",
    })
    renderTree(vi.fn(), { showSessions: true })

    // Anchored: the multi-select checkbox is named "Select Formation room",
    // which a bare /Formation room/ would also match.
    const row = await screen.findByRole("button", { name: /^Formation room/ })
    const fallback = document.querySelector("[data-orphan-rooms]")
    expect(fallback).toBeTruthy()
    expect(fallback?.contains(row)).toBe(true)

    fireEvent.click(row)
    await waitFor(() => {
      expect(h.openRoom).toHaveBeenCalledWith(
        expect.objectContaining({ id: "rm_orphan" })
      )
    })
  })

  it("also rescues a Room bound to a Path the tree does not render", async () => {
    h.rooms.push({
      id: "rm_closed_path",
      workbenchId: 1,
      title: "Closed path room",
      createdByConversationId: 102,
      collectionId: null,
      rootFolderId: 404,
      memberCount: 2,
      unreadCount: 0,
      createdAt: "2026-06-04T00:00:00.000Z",
      updatedAt: "2026-06-04T00:00:00.000Z",
    })
    renderTree(vi.fn(), { showSessions: true })

    const row = await screen.findByRole("button", { name: /^Closed path room/ })
    expect(document.querySelector("[data-orphan-rooms]")?.contains(row)).toBe(
      true
    )
  })

  it("collapses the Path-less Room group without losing the tree", async () => {
    h.rooms.push({
      id: "rm_orphan",
      workbenchId: 1,
      title: "Formation room",
      createdByConversationId: 102,
      collectionId: null,
      rootFolderId: null,
      memberCount: 2,
      unreadCount: 0,
      createdAt: "2026-06-04T00:00:00.000Z",
      updatedAt: "2026-06-04T00:00:00.000Z",
    })
    const { user } = renderTree(vi.fn(), { showSessions: true })
    await screen.findByRole("button", { name: /^Formation room/ })

    const header = document
      .querySelector("[data-orphan-rooms]")
      ?.querySelector<HTMLElement>('button[aria-label="Collapse collection"]')
    await user.click(header!)

    expect(screen.queryByRole("button", { name: /^Formation room/ })).toBeNull()
    expect(document.querySelector("[data-orphan-rooms]")).toBeTruthy()
  })

  it("leaves a Room on a rendered Path out of the fallback group", async () => {
    h.rooms.push({
      id: "rm_path",
      workbenchId: 1,
      title: "Path room",
      createdByConversationId: 102,
      collectionId: null,
      rootFolderId: 7,
      memberCount: 2,
      unreadCount: 0,
      createdAt: "2026-06-04T00:00:00.000Z",
      updatedAt: "2026-06-04T00:00:00.000Z",
    })
    renderTree(vi.fn(), { showSessions: true })

    await screen.findByRole("button", { name: /^Path room/ })
    expect(document.querySelector("[data-orphan-rooms]")).toBeNull()
  })

  it("opens the Collection menu from the expand chevron, not the browser menu", async () => {
    renderTree()
    const expand = screen.getAllByRole("button", {
      name: "Expand collection",
    })[0]
    fireEvent.contextMenu(expand)
    expect(
      await screen.findByRole("menuitem", { name: "New nested collection" })
    ).toBeTruthy()
  })

  it("keeps Collection Rooms in created order when one is newer-updated", async () => {
    h.rooms.push(
      {
        id: "rm_opened",
        workbenchId: 1,
        title: "Opened room",
        createdByConversationId: 102,
        collectionId: 11,
        rootFolderId: 7,
        memberCount: 2,
        unreadCount: 0,
        createdAt: "2026-06-01T00:00:00.000Z",
        updatedAt: "2026-08-19T12:00:00.000Z",
        lastEventAt: "2026-06-01T01:00:00.000Z",
      },
      {
        id: "rm_later",
        workbenchId: 1,
        title: "Later room",
        createdByConversationId: 102,
        collectionId: 11,
        rootFolderId: 7,
        memberCount: 2,
        unreadCount: 0,
        createdAt: "2026-06-04T00:00:00.000Z",
        updatedAt: "2026-06-04T00:00:00.000Z",
        lastEventAt: "2026-06-04T00:00:00.000Z",
      }
    )
    const { user } = renderTree(vi.fn(), { showSessions: true })
    await user.click(screen.getByRole("button", { name: "Research" }))
    await user.click(screen.getByTitle("Sources"))
    const later = await screen.findByRole("button", { name: "Later room" })
    const opened = screen.getByRole("button", { name: "Opened room" })
    expect(
      later.compareDocumentPosition(opened) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy()
  })

  it("opens a Room from its Collection context menu", async () => {
    h.rooms.push({
      id: "rm_menu",
      workbenchId: 1,
      title: "Menu room",
      createdByConversationId: 102,
      collectionId: null,
      rootFolderId: 7,
      memberCount: 2,
      unreadCount: 0,
      createdAt: "2026-06-04T00:00:00.000Z",
      updatedAt: "2026-06-04T00:00:00.000Z",
    })
    renderTree(vi.fn(), { showSessions: true })
    fireEvent.contextMenu(
      await screen.findByRole("button", { name: "Menu room" })
    )
    await userEvent.click(screen.getByRole("menuitem", { name: "Open room" }))
    await waitFor(() => {
      expect(h.openRoom).toHaveBeenCalledWith(
        expect.objectContaining({ id: "rm_menu" })
      )
    })
  })

  it("uses another Session's Collection as the Session drop target", () => {
    expect(
      canDropSessionOnTarget(
        {
          kind: "session",
          conversationId: 102,
          rootFolderId: 7,
          label: "Loose notes",
        },
        7,
        11,
        null
      )
    ).toBe(true)
  })
})

describe("nearestRootFolderId", () => {
  const collection = (
    id: number,
    parentId: number | null,
    rootFolderId: number | null
  ): CollectionInfo => ({
    id,
    root_folder_id: rootFolderId,
    parent_id: parentId,
    name: `Collection ${id}`,
    position: 0,
    created_at: "2026-06-01T00:00:00.000Z",
    updated_at: "2026-06-01T00:00:00.000Z",
  })

  it("returns a nested Collection's own canonical Path", () => {
    expect(nearestRootFolderId(h.items, 11)).toBe(7)
  })

  it("walks up to the nearest ancestor owned by a canonical Path", () => {
    const items = [
      collection(1, null, 7),
      collection(2, 1, null),
      collection(3, 2, null),
    ]
    expect(nearestRootFolderId(items, 3)).toBe(7)
  })

  it("returns null when no ancestor owns a canonical Path", () => {
    const items = [collection(1, null, null), collection(2, 1, null)]
    expect(nearestRootFolderId(items, 2)).toBeNull()
  })

  it("stops on a parent cycle instead of looping", () => {
    const items = [collection(1, 2, null), collection(2, 1, null)]
    expect(nearestRootFolderId(items, 1)).toBeNull()
  })

  it("returns null for an unknown Collection id", () => {
    expect(nearestRootFolderId(h.items, 999)).toBeNull()
  })
})
