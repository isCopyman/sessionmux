import { beforeEach, describe, expect, it, vi } from "vitest"

import type { DbConversationSummary } from "@/lib/types"

const api = vi.hoisted(() => ({
  assignConversationsToCollection: vi.fn(),
  deleteConversation: vi.fn(),
  updateConversationArchive: vi.fn(),
}))

const store = vi.hoisted(() => ({
  applyConversationUpsert: vi.fn(),
  applyConversationRemove: vi.fn(),
  invalidate: vi.fn(),
}))

vi.mock("@/lib/api", () => api)
vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: {
    getState: () => ({
      applyConversationUpsert: store.applyConversationUpsert,
      applyConversationRemove: store.applyConversationRemove,
    }),
  },
}))
vi.mock("@/stores/organization-revision-store", () => ({
  useOrganizationRevisionStore: {
    getState: () => ({ invalidate: store.invalidate }),
  },
}))

import {
  archiveSessions,
  deleteSessions,
  moveSessionsToCollection,
} from "./session-bulk-operations"

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

describe("session bulk operations", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    api.updateConversationArchive.mockResolvedValue(undefined)
    api.deleteConversation.mockResolvedValue(undefined)
    api.assignConversationsToCollection.mockResolvedValue([])
  })

  it("archives each session and patches the workspace store", async () => {
    await archiveSessions([conversation(1), conversation(2)])
    expect(api.updateConversationArchive).toHaveBeenCalledWith(1, true)
    expect(api.updateConversationArchive).toHaveBeenCalledWith(2, true)
    expect(store.applyConversationUpsert).toHaveBeenCalledTimes(2)
    expect(store.applyConversationUpsert).toHaveBeenCalledWith(
      expect.objectContaining({ id: 1, archived_at: expect.any(String) })
    )
  })

  it("deletes each session, closes its tab, and removes it locally", async () => {
    const closeConversationTab = vi.fn()
    await deleteSessions(
      [conversation(4), conversation(5)],
      closeConversationTab
    )
    expect(api.deleteConversation).toHaveBeenCalledWith(4)
    expect(closeConversationTab).toHaveBeenCalledWith(7, 5, "codex")
    expect(store.applyConversationRemove).toHaveBeenCalledWith(5)
  })

  it("moves sessions and invalidates collection membership", async () => {
    await moveSessionsToCollection([8, 9], 11)
    expect(api.assignConversationsToCollection).toHaveBeenCalledWith([8, 9], 11)
    expect(store.invalidate).toHaveBeenCalled()
  })
})
