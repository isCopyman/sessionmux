import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { CollectionTree } from "./collection-tree"
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
}))

vi.mock("@/lib/api", () => ({
  listConversationCollectionRefs: h.listRefs,
  assignConversationsToCollection: h.assignCollection,
}))

function createDataTransfer() {
  const values = new Map<string, string>()
  const types: string[] = []
  return {
    dropEffect: "none",
    effectAllowed: "none",
    files: [],
    items: [],
    types,
    clearData: vi.fn((type?: string) => {
      if (type) values.delete(type)
      else values.clear()
    }),
    getData: vi.fn((type: string) => values.get(type) ?? ""),
    setData: vi.fn((type: string, value: string) => {
      values.set(type, value)
      if (!types.includes(type)) types.push(type)
    }),
    setDragImage: vi.fn(),
  } as unknown as DataTransfer
}

function fireDragAt(
  target: Element,
  type: "dragover" | "drop",
  dataTransfer: DataTransfer,
  clientY: number
) {
  const event = new Event(type, { bubbles: true, cancelable: true })
  Object.defineProperties(event, {
    dataTransfer: { value: dataTransfer },
    clientY: { value: clientY },
  })
  fireEvent(target, event)
}

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
  } = {}
) {
  render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <CollectionTree onOpenScope={onOpenScope} {...options} />
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
    const research = screen.getByRole("button", { name: "Research" })
    await user.click(research.previousElementSibling as HTMLElement)
    await user.click(screen.getByTitle("Sources"))

    expect(onOpenScope).toHaveBeenCalledWith(11)
  })

  it("creates a top-level Collection without touching execution folders", async () => {
    const { user, onOpenScope } = renderTree()
    await user.click(screen.getByRole("button", { name: "New collection" }))
    await user.type(screen.getByPlaceholderText("Collection name"), "Writing")
    await user.click(screen.getByRole("button", { name: "Confirm" }))

    expect(h.create).toHaveBeenCalledWith("Writing", null, 7)
    expect(onOpenScope).toHaveBeenCalledWith(14)
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

  it("moves a Session into a Collection without changing its Path", async () => {
    renderTree(vi.fn(), { showSessions: true })
    const source = (await screen.findByText("Loose notes")).closest("button")!
    const target = document.querySelector('[data-collection-id="10"]')!
    const dataTransfer = createDataTransfer()

    fireEvent.dragStart(source, { dataTransfer })
    fireEvent.dragOver(target, { dataTransfer })
    expect(target.getAttribute("data-session-drop-target")).toBe("true")
    fireEvent.drop(target, { dataTransfer })

    await waitFor(() =>
      expect(h.assignCollection).toHaveBeenCalledWith([102], 10)
    )
    await waitFor(() =>
      expect(
        document.querySelector('[data-collection-id="10"]')?.parentElement
          ?.textContent
      ).toContain("Loose notes")
    )
  })

  it("moves a classified Session back to Unclassified", async () => {
    const { user } = renderTree(vi.fn(), { showSessions: true })
    await user.click(screen.getByRole("button", { name: "Research" }))
    await user.click(screen.getByRole("button", { name: "Sources" }))
    const source = (await screen.findByText("Evidence review")).closest(
      "button"
    )!
    const target = document.querySelector('[data-unclassified-root-id="7"]')!
    const dataTransfer = createDataTransfer()

    fireEvent.dragStart(source, { dataTransfer })
    fireEvent.dragOver(target, { dataTransfer })
    fireEvent.drop(target, { dataTransfer })

    await waitFor(() =>
      expect(h.assignCollection).toHaveBeenCalledWith([101], null)
    )
    await waitFor(() =>
      expect(target.parentElement?.textContent).toContain("Evidence review")
    )
  })

  it("does not move a Session to its current Collection", async () => {
    const { user } = renderTree(vi.fn(), { showSessions: true })
    await user.click(screen.getByRole("button", { name: "Research" }))
    await user.click(screen.getByRole("button", { name: "Sources" }))
    const source = (await screen.findByText("Evidence review")).closest(
      "button"
    )!
    const target = document.querySelector('[data-collection-id="11"]')!
    const dataTransfer = createDataTransfer()

    fireEvent.dragStart(source, { dataTransfer })
    fireEvent.dragOver(target, { dataTransfer })
    fireEvent.drop(target, { dataTransfer })

    expect(h.assignCollection).not.toHaveBeenCalled()
    expect(target.getAttribute("data-session-drop-target")).toBeNull()
  })

  it("rejects Collection drops across canonical Paths", async () => {
    renderTree(vi.fn(), { showSessions: true })
    const source = (await screen.findByText("Loose notes")).closest("button")!
    const target = document.querySelector('[data-collection-id="12"]')!
    const dataTransfer = createDataTransfer()

    fireEvent.dragStart(source, { dataTransfer })
    fireEvent.dragOver(target, { dataTransfer })
    fireEvent.drop(target, { dataTransfer })

    expect(dataTransfer.dropEffect).toBe("none")
    expect(h.assignCollection).not.toHaveBeenCalled()
    expect(target.getAttribute("data-session-drop-target")).toBeNull()
  })

  it("reorders Collections with an explicit before insertion line", async () => {
    renderTree(vi.fn(), { showSessions: true })
    const source = screen.getByRole("button", { name: "Writing" })
    const target = document.querySelector('[data-collection-id="10"]')!
    vi.spyOn(target, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 100,
      top: 100,
      left: 0,
      right: 220,
      bottom: 128,
      width: 220,
      height: 28,
      toJSON: () => ({}),
    })
    const dataTransfer = createDataTransfer()

    fireEvent.dragStart(source, { dataTransfer })
    fireDragAt(target, "dragover", dataTransfer, 102)
    expect(target.getAttribute("data-collection-drop-position")).toBe("before")
    fireDragAt(target, "drop", dataTransfer, 102)

    await waitFor(() => expect(h.place).toHaveBeenCalledWith(13, null, 0))
  })

  it("nests a Collection when dropped in the center of another row", async () => {
    renderTree(vi.fn(), { showSessions: true })
    const source = screen.getByRole("button", { name: "Writing" })
    const target = document.querySelector('[data-collection-id="10"]')!
    vi.spyOn(target, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 100,
      top: 100,
      left: 0,
      right: 220,
      bottom: 128,
      width: 220,
      height: 28,
      toJSON: () => ({}),
    })
    const dataTransfer = createDataTransfer()

    fireEvent.dragStart(source, { dataTransfer })
    fireDragAt(target, "dragover", dataTransfer, 114)
    expect(target.getAttribute("data-collection-drop-position")).toBe("inside")
    fireDragAt(target, "drop", dataTransfer, 114)

    await waitFor(() => expect(h.place).toHaveBeenCalledWith(13, 10, 1))
  })

  it("promotes a nested Collection by dropping it on the Path header", async () => {
    const { user } = renderTree(vi.fn(), { showSessions: true })
    await user.click(screen.getByRole("button", { name: "Research" }))
    const source = screen.getByRole("button", { name: "Sources" })
    const target = document.querySelector('[data-collection-path="7"] > div')!
    const dataTransfer = createDataTransfer()

    fireEvent.dragStart(source, { dataTransfer })
    fireEvent.dragOver(target, { dataTransfer })
    expect(target.getAttribute("data-collection-root-drop")).toBe("true")
    fireEvent.drop(target, { dataTransfer })

    await waitFor(() => expect(h.place).toHaveBeenCalledWith(11, null, 2))
  })

  it("rejects Collection cycles and cross-Path placement", async () => {
    const { user } = renderTree(vi.fn(), { showSessions: true })
    await user.click(screen.getByRole("button", { name: "Research" }))
    const dataTransfer = createDataTransfer()
    const research = screen.getByRole("button", { name: "Research" })
    const sources = document.querySelector('[data-collection-id="11"]')!
    const other = document.querySelector('[data-collection-id="12"]')!

    fireEvent.dragStart(research, { dataTransfer })
    fireEvent.dragOver(sources, { dataTransfer })
    fireEvent.drop(sources, { dataTransfer })
    fireEvent.dragOver(other, { dataTransfer })
    fireEvent.drop(other, { dataTransfer })

    expect(h.place).not.toHaveBeenCalled()
    expect(sources.getAttribute("data-collection-drop-position")).toBeNull()
    expect(other.getAttribute("data-collection-drop-position")).toBeNull()
  })
})
