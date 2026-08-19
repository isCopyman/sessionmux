import { describe, expect, it } from "vitest"

import {
  nextKeySelection,
  rangeKeys,
  uniqueOrderedKeys,
} from "./session-multi-select"
import {
  itemKeyOf,
  parseItemKey,
  roomItemKey,
  selectionRooms,
  selectionSessions,
  sessionItemKey,
  type SidebarSelectionItem,
} from "./sidebar-item-selection"
import type { CollaborationRoomSummary, DbConversationSummary } from "@/lib/types"

function session(id: number): DbConversationSummary {
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

const sessionItem = (id: number): SidebarSelectionItem => ({
  kind: "session",
  session: session(id),
})
const roomItem = (id: string): SidebarSelectionItem => ({
  kind: "room",
  room: room(id),
})

describe("item keys", () => {
  it("prefixes the two id spaces so a Room id can never collide with a Session id", () => {
    expect(sessionItemKey(7)).toBe("session:7")
    expect(roomItemKey("rm_7")).toBe("room:rm_7")
    // A Room literally named "7" still cannot alias Session 7.
    expect(roomItemKey("7")).not.toBe(sessionItemKey(7))
  })

  it("round-trips through parseItemKey", () => {
    expect(parseItemKey(sessionItemKey(7))).toEqual({ kind: "session", id: 7 })
    expect(parseItemKey(roomItemKey("rm_abc"))).toEqual({
      kind: "room",
      id: "rm_abc",
    })
  })

  it("rejects keys without a known prefix or with a non-integer Session id", () => {
    expect(parseItemKey("7")).toBeNull()
    expect(parseItemKey("session:abc")).toBeNull()
  })

  it("derives the key from the item", () => {
    expect(itemKeyOf(sessionItem(3))).toBe("session:3")
    expect(itemKeyOf(roomItem("rm_x"))).toBe("room:rm_x")
  })
})

describe("selectionSessions / selectionRooms", () => {
  it("split a mixed selection by kind, preserving selection order", () => {
    const selected = new Map<string, SidebarSelectionItem>([
      ["session:1", sessionItem(1)],
      ["room:rm_a", roomItem("rm_a")],
      ["session:2", sessionItem(2)],
    ])
    expect(selectionSessions(selected).map((item) => item.id)).toEqual([1, 2])
    expect(selectionRooms(selected).map((item) => item.id)).toEqual(["rm_a"])
  })
})

describe("nextKeySelection with mixed items", () => {
  const ordered = [
    sessionItemKey(1),
    roomItemKey("rm_a"),
    sessionItemKey(2),
    roomItemKey("rm_b"),
  ]
  const items = new Map<string, SidebarSelectionItem>([
    [sessionItemKey(1), sessionItem(1)],
    [roomItemKey("rm_a"), roomItem("rm_a")],
    [sessionItemKey(2), sessionItem(2)],
    [roomItemKey("rm_b"), roomItem("rm_b")],
  ])
  const lookup = (key: string) => items.get(key)

  it("toggles a Room into a Session selection", () => {
    const current = new Map([[sessionItemKey(1), sessionItem(1)]])
    const next = nextKeySelection({
      current,
      orderedKeys: ordered,
      lookup,
      clickedKey: roomItemKey("rm_a"),
      clicked: roomItem("rm_a"),
      intent: "toggle",
      anchorKey: sessionItemKey(1),
    })
    expect([...next.selected.keys()]).toEqual([
      sessionItemKey(1),
      roomItemKey("rm_a"),
    ])
  })

  it("ranges across kinds from the anchor", () => {
    const next = nextKeySelection({
      current: new Map(),
      orderedKeys: ordered,
      lookup,
      clickedKey: roomItemKey("rm_b"),
      clicked: roomItem("rm_b"),
      intent: "range",
      anchorKey: sessionItemKey(1),
    })
    expect([...next.selected.keys()]).toEqual(ordered)
  })

  it("clears the whole mixed selection on a plain open click", () => {
    const current = new Map([
      [sessionItemKey(1), sessionItem(1)],
      [roomItemKey("rm_a"), roomItem("rm_a")],
    ])
    const next = nextKeySelection({
      current,
      orderedKeys: ordered,
      lookup,
      clickedKey: sessionItemKey(2),
      clicked: sessionItem(2),
      intent: "open",
      anchorKey: roomItemKey("rm_a"),
    })
    expect(next.selected.size).toBe(0)
    expect(next.anchorKey).toBe(sessionItemKey(2))
  })
})

describe("generic key helpers", () => {
  it("uniqueOrderedKeys keeps first-seen order for string keys", () => {
    expect(uniqueOrderedKeys(["b", "a", "b", "c"])).toEqual(["b", "a", "c"])
  })

  it("rangeKeys works over mixed string keys", () => {
    const ordered = ["session:1", "room:rm_a", "session:2"]
    expect(rangeKeys(ordered, "session:1", "session:2")).toEqual(ordered)
    expect(rangeKeys(ordered, null, "room:rm_a")).toEqual(["room:rm_a"])
  })
})
