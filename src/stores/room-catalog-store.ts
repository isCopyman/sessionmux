import { create } from "zustand"

import { listWorkbenchRooms } from "@/lib/api"
import { subscribe } from "@/lib/platform"
import { ROOM_CHANGED_EVENT } from "@/lib/room-events"
import type { CollaborationRoomSummary, RoomChanged } from "@/lib/types"
import { useWorkbenchStore } from "@/stores/workbench-store"

interface RoomCatalogState {
  rooms: CollaborationRoomSummary[]
  hydrated: boolean
  refresh: () => Promise<void>
}

async function loadAllRooms(): Promise<CollaborationRoomSummary[]> {
  const workbenches = useWorkbenchStore.getState().items
  const ids = workbenches.length > 0 ? workbenches.map((item) => item.id) : [1]
  const lists = await Promise.all(
    ids.map((workbenchId) => listWorkbenchRooms(workbenchId))
  )
  const byId = new Map<string, CollaborationRoomSummary>()
  for (const list of lists) {
    for (const room of list) byId.set(room.id, room)
  }
  return [...byId.values()].sort(
    (left, right) =>
      Date.parse(right.updatedAt) - Date.parse(left.updatedAt) ||
      left.id.localeCompare(right.id)
  )
}

export const useRoomCatalogStore = create<RoomCatalogState>((set) => ({
  rooms: [],
  hydrated: false,
  refresh: async () => {
    const rooms = await loadAllRooms()
    set({ rooms, hydrated: true })
  },
}))

let subscribed = false

export function ensureRoomCatalogSubscription() {
  if (subscribed || typeof window === "undefined") return
  subscribed = true
  void subscribe<RoomChanged>(ROOM_CHANGED_EVENT, () => {
    void useRoomCatalogStore.getState().refresh()
  })
}
