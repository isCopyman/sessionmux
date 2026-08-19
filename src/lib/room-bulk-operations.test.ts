import { beforeEach, describe, expect, it, vi } from "vitest"

import type { CollaborationRoomSummary } from "@/lib/types"

const api = vi.hoisted(() => ({
  assignRoomsToCollection: vi.fn(),
  deleteCollaborationRoom: vi.fn(),
}))

const catalog = vi.hoisted(() => ({
  refresh: vi.fn(),
}))

vi.mock("@/lib/api", () => api)
vi.mock("@/stores/room-catalog-store", () => ({
  useRoomCatalogStore: { getState: () => ({ refresh: catalog.refresh }) },
}))
vi.mock("@/stores/tab-store", () => ({
  makeRoomTabId: (roomId: string) => `room:${roomId}`,
}))
vi.mock("@/lib/open-room", () => ({
  ROOM_TAB_PLACEHOLDER_AGENT: "claude_code",
  roomTabFolderId: (
    room: { rootFolderId?: number | null },
    folders: { id: number }[]
  ) => room.rootFolderId ?? folders[0]?.id ?? 1,
}))

import {
  collectionsAllowedForRooms,
  deleteRooms,
  moveRoomsToCollection,
  openRoomsInCurrentWorkbench,
} from "./room-bulk-operations"

function room(
  id: string,
  rootFolderId: number | null = null
): CollaborationRoomSummary {
  return {
    id,
    workbenchId: 1,
    title: `Room ${id}`,
    createdByConversationId: 1,
    memberCount: 2,
    unreadCount: 0,
    createdAt: "2026-06-01T00:00:00.000Z",
    updatedAt: "2026-06-01T00:00:00.000Z",
    ...(rootFolderId != null ? { rootFolderId } : {}),
  }
}

describe("collectionsAllowedForRooms", () => {
  const collections = [
    { id: 10, root_folder_id: 7 },
    { id: 11, root_folder_id: 8 },
    { id: 12, root_folder_id: null },
  ]

  it("keeps every Collection when no selected Room is rooted", () => {
    expect(collectionsAllowedForRooms(collections, [room("rm_a")])).toEqual(
      collections
    )
  })

  it("keeps only Collections on every selected Room's Path root", () => {
    const allowed = collectionsAllowedForRooms(collections, [
      room("rm_a", 7),
      room("rm_b", 7),
    ])
    expect(allowed.map((item) => item.id)).toEqual([10])
  })

  it("intersects roots across Rooms, so a cross-root selection empties the list", () => {
    const allowed = collectionsAllowedForRooms(collections, [
      room("rm_a", 7),
      room("rm_b", 8),
    ])
    expect(allowed).toEqual([])
  })
})

describe("moveRoomsToCollection", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    api.assignRoomsToCollection.mockResolvedValue(undefined)
  })

  it("assigns all Room ids in one call and leaves rootFolderId to the backend COALESCE", async () => {
    await moveRoomsToCollection([room("rm_a"), room("rm_b")], 10)
    expect(api.assignRoomsToCollection).toHaveBeenCalledWith(
      ["rm_a", "rm_b"],
      10,
      null
    )
    expect(catalog.refresh).toHaveBeenCalledTimes(1)
  })

  it("does nothing for an empty selection", async () => {
    await moveRoomsToCollection([], 10)
    expect(api.assignRoomsToCollection).not.toHaveBeenCalled()
    expect(catalog.refresh).not.toHaveBeenCalled()
  })
})

describe("deleteRooms", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    api.deleteCollaborationRoom.mockResolvedValue(undefined)
  })

  it("deletes every Room, closes its tab, then refreshes the catalog", async () => {
    const closeTab = vi.fn()
    await deleteRooms([room("rm_a"), room("rm_b")], closeTab)
    expect(api.deleteCollaborationRoom).toHaveBeenCalledWith("rm_a")
    expect(api.deleteCollaborationRoom).toHaveBeenCalledWith("rm_b")
    expect(closeTab).toHaveBeenCalledWith("room:rm_a")
    expect(closeTab).toHaveBeenCalledWith("room:rm_b")
    expect(catalog.refresh).toHaveBeenCalledTimes(1)
  })

  it("does nothing for an empty selection", async () => {
    const closeTab = vi.fn()
    await deleteRooms([], closeTab)
    expect(api.deleteCollaborationRoom).not.toHaveBeenCalled()
    expect(closeTab).not.toHaveBeenCalled()
  })
})

describe("openRoomsInCurrentWorkbench", () => {
  it("pins only the Rooms not already open, as placeholder-agent room tabs", () => {
    const openRoomTab = vi.fn()
    const result = openRoomsInCurrentWorkbench({
      rooms: [room("rm_a"), room("rm_b")],
      openTabRoomIds: new Set(["rm_a"]),
      openRoomTab,
      folders: [{ id: 4 }],
    })
    expect(result).toEqual({ added: 1, skipped: 1 })
    expect(openRoomTab).toHaveBeenCalledTimes(1)
    expect(openRoomTab).toHaveBeenCalledWith({
      roomId: "rm_b",
      title: "Room rm_b",
      folderId: 4,
      agentType: "claude_code",
    })
  })

  it("reports an all-skipped selection without opening anything", () => {
    const openRoomTab = vi.fn()
    const result = openRoomsInCurrentWorkbench({
      rooms: [room("rm_a")],
      openTabRoomIds: new Set(["rm_a"]),
      openRoomTab,
      folders: [{ id: 4 }],
    })
    expect(result).toEqual({ added: 0, skipped: 1 })
    expect(openRoomTab).not.toHaveBeenCalled()
  })
})
