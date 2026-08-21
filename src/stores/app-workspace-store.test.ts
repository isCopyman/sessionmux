import { beforeEach, describe, expect, it, vi } from "vitest"
import {
  resetAppWorkspaceStore,
  useAppWorkspaceStore,
} from "./app-workspace-store"
import type { DbConversationSummary, FolderDetail } from "@/lib/types"

vi.mock("@/lib/api", () => ({
  getFolder: vi.fn(),
  listOpenFolderDetails: vi.fn(async () => []),
  listAllFolderDetails: vi.fn(async () => []),
  listAllConversations: vi.fn(async () => []),
  openFolder: vi.fn(),
  openFolderById: vi.fn(),
  openWorktreeFolder: vi.fn(),
  removeFolderFromWorkspace: vi.fn(),
  reorderFolders: vi.fn(),
}))

const {
  getFolder,
  listAllFolderDetails,
  listOpenFolderDetails,
  openFolder,
  openFolderById,
  openWorktreeFolder,
} = await import("@/lib/api")
const mockGetFolder = vi.mocked(getFolder)
const mockListAllFolders = vi.mocked(listAllFolderDetails)
const mockListOpenFolders = vi.mocked(listOpenFolderDetails)
const mockOpenFolder = vi.mocked(openFolder)
const mockOpenFolderById = vi.mocked(openFolderById)
const mockOpenWorktreeFolder = vi.mocked(openWorktreeFolder)

function makeSummary(
  overrides: Partial<DbConversationSummary> & { id: number }
): DbConversationSummary {
  return {
    folder_id: 1,
    title: null,
    title_locked: false,
    agent_type: "claude_code",
    status: "in_progress",
    kind: "regular",
    model: null,
    git_branch: null,
    external_id: null,
    message_count: 0,
    child_count: 0,
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
    pinned_at: null,
    parent_id: null,
    parent_tool_use_id: null,
    delegation_call_id: null,
    ...overrides,
  }
}

beforeEach(() => {
  resetAppWorkspaceStore()
})

describe("updateConversationLocal — stats reference stability", () => {
  function seedTwo() {
    const store = useAppWorkspaceStore.getState()
    store.applyConversationUpsert(makeSummary({ id: 1, message_count: 3 }))
    store.applyConversationUpsert(makeSummary({ id: 2, message_count: 4 }))
  }

  it("reuses the stats reference on a status patch (no stat can change)", () => {
    seedTwo()
    const before = useAppWorkspaceStore.getState()
    const statsBefore = before.stats
    const conversationsBefore = before.conversations

    useAppWorkspaceStore
      .getState()
      .updateConversationLocal(1, { status: "pending_review" })

    const after = useAppWorkspaceStore.getState()
    // The regression guard: a turn-boundary status flip must NOT mint a fresh
    // `stats` object (which would re-render every stats subscriber for a no-op).
    expect(after.stats).toBe(statsBefore)
    // But the row's data genuinely changed, so `conversations` gets a new ref
    // (sidebar consumers must see the status update).
    expect(after.conversations).not.toBe(conversationsBefore)
    expect(after.conversations.find((c) => c.id === 1)?.status).toBe(
      "pending_review"
    )
  })

  it("reuses the stats reference on a title patch", () => {
    seedTwo()
    const statsBefore = useAppWorkspaceStore.getState().stats

    useAppWorkspaceStore
      .getState()
      .updateConversationLocal(2, { title: "Renamed" })

    const after = useAppWorkspaceStore.getState()
    expect(after.stats).toBe(statsBefore)
    expect(after.conversations.find((c) => c.id === 2)?.title).toBe("Renamed")
  })

  it("patches paused_reason without bumping updated_at or stats", () => {
    seedTwo()
    const before = useAppWorkspaceStore.getState()
    const updatedAt = before.conversations.find((c) => c.id === 1)?.updated_at

    before.updateConversationLocal(1, {
      paused_reason: "cancelled_current_turn",
    })

    const after = useAppWorkspaceStore.getState()
    expect(after.stats).toBe(before.stats)
    expect(after.conversations.find((c) => c.id === 1)?.paused_reason).toBe(
      "cancelled_current_turn"
    )
    expect(after.conversations.find((c) => c.id === 1)?.updated_at).toBe(
      updatedAt
    )
  })

  it("leaves state untouched (stable refs) for an unknown id", () => {
    seedTwo()
    const before = useAppWorkspaceStore.getState()

    before.updateConversationLocal(999, { status: "cancelled" })

    const after = useAppWorkspaceStore.getState()
    expect(after.stats).toBe(before.stats)
    expect(after.conversations).toBe(before.conversations)
  })

  it("still tracks stats when message_count actually changes (via upsert)", () => {
    seedTwo()
    // total_messages = 3 + 4
    expect(useAppWorkspaceStore.getState().stats?.total_messages).toBe(7)

    // A real message_count change flows through applyConversationUpsert (whose
    // recompute we intentionally left intact), so stats update as before.
    useAppWorkspaceStore
      .getState()
      .applyConversationUpsert(makeSummary({ id: 1, message_count: 10 }))

    expect(useAppWorkspaceStore.getState().stats?.total_messages).toBe(14)
  })

  it("hides an archived upsert without blocking a later restore", () => {
    const store = useAppWorkspaceStore.getState()
    store.applyConversationUpsert(makeSummary({ id: 7 }))
    expect(useAppWorkspaceStore.getState().conversations).toHaveLength(1)

    store.applyConversationUpsert(
      makeSummary({ id: 7, archived_at: "2026-08-15T12:00:00.000Z" })
    )
    expect(useAppWorkspaceStore.getState().conversations).toHaveLength(0)

    store.applyConversationUpsert(makeSummary({ id: 7, archived_at: null }))
    expect(
      useAppWorkspaceStore.getState().conversations.map((row) => row.id)
    ).toEqual([7])
  })
})

function makeFolder(
  overrides: Partial<FolderDetail> & { id: number }
): FolderDetail {
  return {
    name: "repo",
    path: "/tmp/repo",
    git_branch: null,
    default_agent_type: null,
    last_opened_at: "2026-01-01T00:00:00.000Z",
    sort_order: 1,
    color: "#000000",
    parent_id: null,
    kind: "regular",
    alias: null,
    ...overrides,
  }
}

describe("refreshFolder — branch null-guard", () => {
  it("keeps the poll-resolved branch when the refreshed row's git_branch is null", async () => {
    // Git-head polling has populated the display branch; the folder row's
    // `git_branch` column is null (it always is today), so the refresh must
    // leave the polled name alone.
    useAppWorkspaceStore.getState().setBranch(1, "feature/x")
    mockGetFolder.mockResolvedValue(makeFolder({ id: 1, git_branch: null }))

    await useAppWorkspaceStore.getState().refreshFolder(1)

    // Regression guard for the "no branch" flash: a null DB branch must not
    // clobber the polled name (which would blank the bottom selector until the
    // next poll, up to 10s later).
    expect(useAppWorkspaceStore.getState().branches.get(1)).toBe("feature/x")
  })

  it("adopts the refreshed branch when the row actually carries one", async () => {
    useAppWorkspaceStore.getState().setBranch(1, "old")
    mockGetFolder.mockResolvedValue(makeFolder({ id: 1, git_branch: "main" }))

    await useAppWorkspaceStore.getState().refreshFolder(1)

    expect(useAppWorkspaceStore.getState().branches.get(1)).toBe("main")
  })
})

describe("openFolder — branch null-guard", () => {
  it("keeps the poll-resolved branch when the opened row's git_branch is null", async () => {
    useAppWorkspaceStore.getState().setBranch(1, "wt/fork-rewind")
    mockOpenFolder.mockResolvedValue(
      makeFolder({ id: 1, git_branch: null, path: "/tmp/wt" })
    )

    await useAppWorkspaceStore.getState().openFolder("/tmp/wt")

    expect(useAppWorkspaceStore.getState().branches.get(1)).toBe(
      "wt/fork-rewind"
    )
  })

  it("adopts the opened branch when the row actually carries one", async () => {
    useAppWorkspaceStore.getState().setBranch(1, "old")
    mockOpenFolder.mockResolvedValue(
      makeFolder({ id: 1, git_branch: "feature/x" })
    )

    await useAppWorkspaceStore.getState().openFolder("/tmp/repo")

    expect(useAppWorkspaceStore.getState().branches.get(1)).toBe("feature/x")
  })

  it("does the same null-guard for openWorktreeFolder and open-by-id", async () => {
    useAppWorkspaceStore.getState().setBranch(2, "wt/o4")
    useAppWorkspaceStore.getState().setBranch(3, "wt/o4-id")
    mockOpenWorktreeFolder.mockResolvedValue(
      makeFolder({ id: 2, git_branch: null, path: "/tmp/wt-2" })
    )
    mockOpenFolderById.mockResolvedValue(
      makeFolder({ id: 3, git_branch: null, path: "/tmp/wt-3" })
    )

    await useAppWorkspaceStore.getState().openWorktreeFolder("/tmp/wt-2", 1)
    await useAppWorkspaceStore.getState().addFolderToWorkspaceById(3)

    expect(useAppWorkspaceStore.getState().branches.get(2)).toBe("wt/o4")
    expect(useAppWorkspaceStore.getState().branches.get(3)).toBe("wt/o4-id")
  })
})

describe("applyFolderRemove", () => {
  it("drops the folder and its branch/HEAD entries from every list", () => {
    const store = useAppWorkspaceStore.getState()
    store.upsertFolder(makeFolder({ id: 1 }))
    store.upsertFolder(makeFolder({ id: 2, parent_id: 1 }))
    store.setBranch(2, "task/7")
    store.applyGitHead(2, {
      is_repo: true,
      branch: "task/7",
      detached: false,
      short_sha: "abc1234",
    })

    useAppWorkspaceStore.getState().applyFolderRemove(2)

    const after = useAppWorkspaceStore.getState()
    expect(after.folders.map((f) => f.id)).toEqual([1])
    expect(after.allFolders.map((f) => f.id)).toEqual([1])
    // Stale branch/HEAD entries would resurface if the id were ever reused.
    expect(after.branches.has(2)).toBe(false)
    expect(after.gitHeads.has(2)).toBe(false)
  })

  it("writes nothing for an unknown id (stable refs, no re-render)", () => {
    useAppWorkspaceStore.getState().upsertFolder(makeFolder({ id: 1 }))
    const before = useAppWorkspaceStore.getState()

    useAppWorkspaceStore.getState().applyFolderRemove(404)

    const after = useAppWorkspaceStore.getState()
    expect(after.folders).toBe(before.folders)
    expect(after.allFolders).toBe(before.allFolders)
    expect(after.branches).toBe(before.branches)
    expect(after.gitHeads).toBe(before.gitHeads)
  })
})

describe("applyFolderRemove — in-flight fetch resurrection guard", () => {
  it("subtracts a removed folder from a snapshot that was already in flight", async () => {
    // Mount / reconnect `fetchFolders` replaces both lists wholesale. A
    // response captured BEFORE the worktree was deleted would otherwise put it
    // straight back on screen.
    const alive = [makeFolder({ id: 1 }), makeFolder({ id: 2, parent_id: 1 })]
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    mockListOpenFolders.mockImplementation(async () => {
      await gate
      return alive
    })
    mockListAllFolders.mockImplementation(async () => {
      await gate
      return alive
    })

    const inFlight = useAppWorkspaceStore.getState().fetchFolders()
    useAppWorkspaceStore.getState().applyFolderRemove(2)
    release()
    await inFlight

    const after = useAppWorkspaceStore.getState()
    expect(after.folders.map((f) => f.id)).toEqual([1])
    expect(after.allFolders.map((f) => f.id)).toEqual([1])
  })

  it("keeps a folder a LATER snapshot still reports (revived while disconnected)", async () => {
    // The reconnect refetch is the reconciliation, and it may be the only place
    // a revive is ever learned: folder ids are reused (a row is revived by path
    // onto the same id), so a task retried after its worktree was cleaned
    // re-creates that exact folder while the socket is down and its upsert
    // event is dropped. Filtering a snapshot requested AFTER the removal would
    // hide that folder forever — and with it every conversation inside it.
    useAppWorkspaceStore.getState().applyFolderRemove(2)

    mockListOpenFolders.mockResolvedValue([makeFolder({ id: 2 })])
    mockListAllFolders.mockResolvedValue([makeFolder({ id: 2 })])
    await useAppWorkspaceStore.getState().fetchFolders()

    expect(useAppWorkspaceStore.getState().folders.map((f) => f.id)).toEqual([
      2,
    ])
    expect(useAppWorkspaceStore.getState().allFolders.map((f) => f.id)).toEqual(
      [2]
    )
  })

  it("lets a later upsert revive the id (a retried task re-creates its worktree)", async () => {
    useAppWorkspaceStore.getState().upsertFolder(makeFolder({ id: 2 }))
    useAppWorkspaceStore.getState().applyFolderRemove(2)
    useAppWorkspaceStore.getState().upsertFolder(makeFolder({ id: 2 }))

    mockListOpenFolders.mockResolvedValue([makeFolder({ id: 2 })])
    mockListAllFolders.mockResolvedValue([makeFolder({ id: 2 })])
    await useAppWorkspaceStore.getState().fetchFolders()

    expect(useAppWorkspaceStore.getState().folders.map((f) => f.id)).toEqual([
      2,
    ])
  })

  it("still filters an in-flight snapshot when a LATER removal is pending", async () => {
    // Two removals, one before the fetch and one during it: only the second may
    // be subtracted, and the first must not smuggle its id back in.
    useAppWorkspaceStore.getState().applyFolderRemove(3)
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    const snapshot = [makeFolder({ id: 1 }), makeFolder({ id: 2 })]
    mockListOpenFolders.mockImplementation(async () => {
      await gate
      return snapshot
    })
    mockListAllFolders.mockImplementation(async () => {
      await gate
      return snapshot
    })

    const inFlight = useAppWorkspaceStore.getState().fetchFolders()
    useAppWorkspaceStore.getState().applyFolderRemove(2)
    release()
    await inFlight

    expect(useAppWorkspaceStore.getState().folders.map((f) => f.id)).toEqual([
      1,
    ])
  })
})
