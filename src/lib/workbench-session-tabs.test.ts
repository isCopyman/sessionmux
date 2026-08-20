import { beforeEach, describe, expect, it, vi } from "vitest"

import type {
  CollaborationRoomSummary,
  ConversationWorkbenchRef,
  DbConversationSummary,
  OpenedTab,
} from "@/lib/types"

const api = vi.hoisted(() => ({
  listOpenedTabs: vi.fn(),
  listWorkbenchTabs: vi.fn(),
  saveOpenedTabs: vi.fn(),
  saveWorkbenchTabs: vi.fn(),
  listConversationWorkbenchRefs: vi.fn(),
}))

vi.mock("@/lib/api", () => api)
vi.mock("@/lib/open-room", () => ({
  ROOM_TAB_PLACEHOLDER_AGENT: "claude_code",
  roomTabFolderId: (
    room: { rootFolderId?: number | null },
    folders: { id: number }[]
  ) => room.rootFolderId ?? folders[0]?.id ?? 1,
}))

import {
  appendConversationTabs,
  appendConversationsToWorkbench,
  appendRoomTabs,
  conversationIdsInTabs,
  conversationIdsOccupiedElsewhere,
  conversationIdsOccupiedElsewhereFor,
  locateConversationHome,
  pickConversationHome,
  roomIdsInTabs,
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

function ref(
  conversationId: number,
  workbenchId: number,
  position = workbenchId
): ConversationWorkbenchRef {
  return {
    conversation_id: conversationId,
    workbench_id: workbenchId,
    workbench_name: `WB ${workbenchId}`,
    workbench_position: position,
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

  it("skips sessions already open on another workbench", () => {
    const result = appendConversationTabs(
      [tab(1, 0)],
      [conversation(2), conversation(3)],
      new Set([2])
    )
    expect(result.added).toBe(1)
    expect(result.skipped).toBe(1)
    expect(result.items.map((item) => item.conversation_id)).toEqual([1, 3])
  })
})

describe("pickConversationHome", () => {
  it("prefers the current workbench when the session is already there", () => {
    expect(
      pickConversationHome([ref(9, 3, 0), ref(9, 1, 2)], 9, 1)?.workbench_id
    ).toBe(1)
  })

  it("picks the earliest-positioned workbench and does not drop extras", () => {
    const homes = [ref(9, 5, 4), ref(9, 2, 1), ref(9, 8, 1)]
    expect(pickConversationHome(homes, 9, 3)?.workbench_id).toBe(2)
    expect(homes).toHaveLength(3)
  })
})

describe("conversationIdsOccupiedElsewhere", () => {
  it("ignores occupancy on the listed workbenches", () => {
    expect(
      conversationIdsOccupiedElsewhere([ref(1, 1), ref(2, 3), ref(1, 2)], [1])
    ).toEqual(new Set([2, 1]))
  })
})

describe("appendConversationsToWorkbench", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    api.listConversationWorkbenchRefs.mockResolvedValue([])
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

    expect(result).toEqual({ added: 1, skipped: 1, addedIds: [9] })
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

  it("does not copy a session that already lives on another workbench", async () => {
    api.listConversationWorkbenchRefs.mockResolvedValue([ref(9, 3)])
    api.listWorkbenchTabs.mockResolvedValue({ items: [], version: 1 })

    const result = await appendConversationsToWorkbench(2, [conversation(9)])

    expect(result).toEqual({ added: 0, skipped: 1, addedIds: [] })
    expect(api.saveWorkbenchTabs).not.toHaveBeenCalled()
  })

  it("still appends when the other occupancy is on an ignored (move source) workbench", async () => {
    api.listConversationWorkbenchRefs.mockResolvedValue([ref(9, 1)])
    api.listWorkbenchTabs.mockResolvedValue({ items: [], version: 1 })
    api.saveWorkbenchTabs.mockResolvedValue({
      accepted: true,
      version: 2,
      tabs: [],
    })

    const result = await appendConversationsToWorkbench(
      2,
      [conversation(9)],
      SESSION_CENTER_TAB_ORIGIN,
      { ignoreWorkbenchIds: [1] }
    )

    expect(result).toEqual({ added: 1, skipped: 0, addedIds: [9] })
    expect(api.saveWorkbenchTabs).toHaveBeenCalled()
  })
})

describe("locateConversationHome", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("returns the home workbench for an already-open session", async () => {
    api.listConversationWorkbenchRefs.mockResolvedValue([ref(4, 7, 2)])
    await expect(locateConversationHome(4, 1)).resolves.toEqual(ref(4, 7, 2))
  })

  it("treats a lookup failure as no home so opening still works", async () => {
    api.listConversationWorkbenchRefs.mockRejectedValue(new Error("offline"))
    await expect(locateConversationHome(4, 1)).resolves.toBeNull()
  })
})

describe("conversationIdsOccupiedElsewhereFor", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("returns an empty set when the occupancy API fails", async () => {
    api.listConversationWorkbenchRefs.mockRejectedValue(new Error("offline"))
    await expect(
      conversationIdsOccupiedElsewhereFor([9], [1])
    ).resolves.toEqual(new Set())
  })
})

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

function roomTab(roomId: string, position: number): OpenedTab {
  return {
    id: position + 1000,
    folder_id: 7,
    conversation_id: null,
    room_id: roomId,
    agent_type: "claude_code",
    position,
    is_active: false,
    is_pinned: true,
  }
}

describe("roomIdsInTabs", () => {
  it("collects room ids, ignoring session-only tabs", () => {
    expect(roomIdsInTabs([tab(3, 0), roomTab("rm_a", 1)])).toEqual(
      new Set(["rm_a"])
    )
  })
})

describe("appendRoomTabs", () => {
  it("appends missing Rooms as pinned placeholder-agent tabs after the last position", () => {
    const result = appendRoomTabs(
      [tab(1, 4)],
      [room("rm_a"), room("rm_b")],
      [{ id: 7 }]
    )
    expect(result).toEqual({
      added: 2,
      skipped: 0,
      items: [
        tab(1, 4),
        {
          id: 0,
          folder_id: 7,
          conversation_id: null,
          room_id: "rm_a",
          agent_type: "claude_code",
          position: 5,
          is_active: false,
          is_pinned: true,
        },
        {
          id: 0,
          folder_id: 7,
          conversation_id: null,
          room_id: "rm_b",
          agent_type: "claude_code",
          position: 6,
          is_active: false,
          is_pinned: true,
        },
      ],
    })
  })

  it("dedupes Rooms already open in the workbench", () => {
    const result = appendRoomTabs(
      [roomTab("rm_a", 0)],
      [room("rm_a"), room("rm_b")],
      [{ id: 7 }]
    )
    expect(result.added).toBe(1)
    expect(result.skipped).toBe(1)
    expect(result.items.map((item) => item.room_id)).toEqual(["rm_a", "rm_b"])
  })

  it("returns the existing list untouched when every Room is already open", () => {
    const existing = [roomTab("rm_a", 0)]
    const result = appendRoomTabs(existing, [room("rm_a")], [{ id: 7 }])
    expect(result).toEqual({ items: existing, added: 0, skipped: 1 })
  })
})
