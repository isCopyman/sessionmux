import { beforeEach, describe, expect, it, vi } from "vitest"

import type { DbConversationSummary, OpenedTab } from "@/lib/types"

const api = vi.hoisted(() => ({
  listOpenedTabs: vi.fn(),
  listWorkbenchTabs: vi.fn(),
  saveOpenedTabs: vi.fn(),
  saveWorkbenchTabs: vi.fn(),
}))

vi.mock("@/lib/api", () => api)

import {
  appendConversationTabs,
  appendConversationsToWorkbench,
  conversationIdsInTabs,
  SESSION_CENTER_TAB_ORIGIN,
} from "./workbench-session-tabs"

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

function tab(conversationId: number, position: number): OpenedTab {
  return {
    id: conversationId,
    folder_id: 7,
    conversation_id: conversationId,
    agent_type: "codex",
    position,
    is_active: false,
    is_pinned: true,
  }
}

describe("appendConversationTabs", () => {
  it("appends missing sessions after the last saved position", () => {
    const result = appendConversationTabs(
      [tab(1, 4)],
      [conversation(1), conversation(2)]
    )
    expect(result).toEqual({
      added: 1,
      skipped: 1,
      items: [
        tab(1, 4),
        {
          id: 0,
          folder_id: 7,
          conversation_id: 2,
          agent_type: "codex",
          position: 5,
          is_active: false,
          is_pinned: true,
        },
      ],
    })
  })

  it("collects conversation ids already present in a workbench", () => {
    expect(
      conversationIdsInTabs([
        tab(3, 0),
        { ...tab(0, 1), conversation_id: null },
      ])
    ).toEqual(new Set([3]))
  })
})

describe("appendConversationsToWorkbench", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("retries once against the latest snapshot when the first save is rejected", async () => {
    api.listWorkbenchTabs.mockResolvedValue({
      items: [tab(1, 0)],
      version: 3,
    })
    api.saveWorkbenchTabs
      .mockResolvedValueOnce({
        accepted: false,
        version: 4,
        tabs: [tab(1, 0), tab(8, 1)],
      })
      .mockResolvedValueOnce({
        accepted: true,
        version: 5,
        tabs: [],
      })

    const result = await appendConversationsToWorkbench(
      2,
      [conversation(1), conversation(9)],
      SESSION_CENTER_TAB_ORIGIN
    )

    expect(result).toEqual({ added: 1, skipped: 1 })
    expect(api.saveWorkbenchTabs).toHaveBeenLastCalledWith(
      2,
      expect.arrayContaining([
        expect.objectContaining({ conversation_id: 9, position: 2 }),
      ]),
      4,
      SESSION_CENTER_TAB_ORIGIN
    )
  })

  it("uses the default opened-tab store for workbench 1", async () => {
    api.listOpenedTabs.mockResolvedValue({ items: [], version: 1 })
    api.saveOpenedTabs.mockResolvedValue({
      accepted: true,
      version: 2,
      tabs: [],
    })

    await appendConversationsToWorkbench(1, [conversation(4)])

    expect(api.listOpenedTabs).toHaveBeenCalled()
    expect(api.saveOpenedTabs).toHaveBeenCalled()
    expect(api.listWorkbenchTabs).not.toHaveBeenCalled()
  })
})
