import { assignRoomsToCollection, deleteCollaborationRoom } from "@/lib/api"
import { ROOM_TAB_PLACEHOLDER_AGENT, roomTabFolderId } from "@/lib/open-room"
import type { AgentType, CollaborationRoomSummary } from "@/lib/types"
import { useRoomCatalogStore } from "@/stores/room-catalog-store"
import { makeRoomTabId } from "@/stores/tab-store"

/**
 * A Collection accepts every selected Room only when it sits on each Room's
 * own Path root — the single-Room move menu in the Collection tree filters
 * its targets the same way. Rooms with no root constrain nothing, so a
 * selection of only rootless Rooms keeps every Collection available.
 */
export function collectionsAllowedForRooms<
  T extends { root_folder_id?: number | null },
>(
  collections: readonly T[],
  rooms: readonly Pick<CollaborationRoomSummary, "rootFolderId">[]
): T[] {
  const roots = new Set<number>()
  for (const room of rooms) {
    if (room.rootFolderId != null) roots.add(room.rootFolderId)
  }
  if (roots.size === 0) return [...collections]
  return collections.filter(
    (item) => item.root_folder_id != null && roots.has(item.root_folder_id)
  )
}

export async function moveRoomsToCollection(
  rooms: readonly CollaborationRoomSummary[],
  collectionId: number | null
): Promise<void> {
  if (rooms.length === 0) return
  // rootFolderId stays null: the backend COALESCEs it with each Room's
  // current root, so a mixed-root selection moves without re-rooting.
  await assignRoomsToCollection(
    rooms.map((room) => room.id),
    collectionId,
    null
  )
  await useRoomCatalogStore.getState().refresh()
}

export async function deleteRooms(
  rooms: readonly CollaborationRoomSummary[],
  closeTab: (tabId: string) => void
): Promise<void> {
  if (rooms.length === 0) return
  await Promise.all(rooms.map((room) => deleteCollaborationRoom(room.id)))
  for (const room of rooms) closeTab(makeRoomTabId(room.id))
  await useRoomCatalogStore.getState().refresh()
}

/**
 * Pin every selected Room as a tab in the CURRENT workbench, skipping Rooms
 * already open there (openRoomTab would just focus them). Mirrors how the
 * bulk bar opens selected Sessions.
 */
export function openRoomsInCurrentWorkbench(args: {
  rooms: readonly CollaborationRoomSummary[]
  openTabRoomIds: ReadonlySet<string>
  openRoomTab: (input: {
    roomId: string
    title: string
    folderId: number
    agentType: AgentType
  }) => void
  folders: readonly { id: number }[]
}): { added: number; skipped: number } {
  const toAdd = args.rooms.filter((room) => !args.openTabRoomIds.has(room.id))
  for (const room of toAdd) {
    args.openRoomTab({
      roomId: room.id,
      title: room.title,
      folderId: roomTabFolderId(room, [...args.folders]),
      agentType: ROOM_TAB_PLACEHOLDER_AGENT,
    })
  }
  return { added: toAdd.length, skipped: args.rooms.length - toAdd.length }
}
