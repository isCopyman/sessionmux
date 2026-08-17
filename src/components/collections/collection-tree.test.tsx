import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { createRef, type RefObject } from "react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { CollectionTree, type CollectionTreeHandle } from "./collection-tree"
import {
  canDropSessionOnTarget,
  collectionPlacementForRoot,
  collectionPlacementForRow,
} from "./collection-tree-dnd"
import enMessages from "@/i18n/messages/en.json"
import type { DbConversationSummary } from "@/lib/types"

const h = vi.hoisted(() => ({
  create: vi.fn(),
  rename: vi.fn(),
  move: vi.fn(),
  place: vi.fn(),
  remove: vi.fn(),
  hydrate: vi.fn(),
  listRefs: vi.fn(),
  assignCollection: vi.fn(),
  updateTitle: vi.fn(),
  updateArchive: vi.fn(),
  updatePinned: vi.fn(),
  updateStatus: vi.fn(),
  deleteConversation: vi.fn(),
  updateConversationLocal: vi.fn(),
  applyConversationUpsert: vi.fn(),
  applyConversationRemove: vi.fn(),
  closeConversationTab: vi.fn(),
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
    },
  ],
  items: [
    {
      id: 10,
      root_folder_id: 7,
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

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: unknown) => unknown) =>
    selector({
      conversations: h.conversations,
      activeFolderId: 7,
      folders: [
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
      ],
      updateConversationLocal: h.updateConversationLocal,
      applyConversationUpsert: h.applyConversationUpsert,
      applyConversationRemove: h.applyConversationRemove,
      allFolders: [
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
      ],
    }),
}))

vi.mock("@/contexts/tab-context", () => ({
  useTabStore: (selector: (state: unknown) => unknown) =>
    selector({
      activeTabId: "conv-102",
      tabs: [{ id: "conv-102", conversationId: 102 }],
    }),
  useTabActions: () => ({
    closeConversationTab: h.closeConversationTab,
  }),
}))

vi.mock("@/components/conversations/session-details-dialog", () => ({
  SessionDetailsDialog: () => null,
}))

vi.mock("@/lib/api", () => ({
  listConversationCollectionRefs: h.listRefs,
  assignConversationsToCollection: h.assignCollection,
  updateConversationTitle: h.updateTitle,
  updateConversationArchive: h.updateArchive,
  updateConversationPinned: h.updatePinned,
  updateConversationStatus: h.updateStatus,
  deleteConversation: h.deleteConversation,
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
    onOpenSession?: (session: DbConversationSummary) => void
    onOpenSessionInSplit?: (
      session: DbConversationSummary,
      direction: "right" | "down"
    ) => void
    onNewSession?: (rootFolderId: number) => void
    treeRef?: RefObject<CollectionTreeHandle | null>
  } = {}
) {
  const { treeRef, ...treeOptions } = options
  render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <CollectionTree
        ref={treeRef}
        onOpenScope={onOpenScope}
        {...treeOptions}
      />
    </NextIntlClientProvider>
  )
  return { user: userEvent.setup(), onOpenScope }
}

describe("CollectionTree", () => {
  beforeEach(() => {
    vi.clearAllMocks()
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
