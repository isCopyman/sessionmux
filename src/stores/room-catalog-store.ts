import { create } from "zustand"

import { listWorkbenchRooms } from "@/lib/api"
import { subscribe } from "@/lib/platform"
import { ROOM_CHANGED_EVENT } from "@/lib/room-events"
import { sortRoomsForSidebar } from "@/lib/room-sidebar-order"
import type { CollaborationRoomSummary, RoomChanged } from "@/lib/types"
import { useWorkbenchStore } from "@/stores/workbench-store"

interface RoomCatalogState {
  rooms: CollaborationRoomSummary[]
  hydrated: boolean
  refresh: () => Promise<void>
}

function workbenchIdsKey(items: { id: number }[]): string {
  return items.map((item) => item.id).join(",")
}

async function loadAllRooms(): Promise<CollaborationRoomSummary[]> {
  const workbenches = useWorkbenchStore.getState().items
  // Do not fall back to workbench 1. An empty list means Workbenches have not
  // hydrated yet; claiming "no rooms" from Main alone hides every other
  // Workbench's Rooms until the next manual refresh.
  if (workbenches.length === 0) return []
  const lists = await Promise.all(
    workbenches.map((item) => listWorkbenchRooms(item.id))
  )
  const byId = new Map<string, CollaborationRoomSummary>()
  for (const list of lists) {
    for (const room of list) byId.set(room.id, room)
  }
  return sortRoomsForSidebar([...byId.values()], "created")
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
let lastWorkbenchKey = ""
let workbenchBound = false

function bindWorkbenchCatalog() {
  if (workbenchBound || typeof window === "undefined") return
  workbenchBound = true
  lastWorkbenchKey = workbenchIdsKey(useWorkbenchStore.getState().items)
  useWorkbenchStore.subscribe((state) => {
    const next = workbenchIdsKey(state.items)
    if (next === lastWorkbenchKey) return
    lastWorkbenchKey = next
    void useRoomCatalogStore.getState().refresh()
  })
}

export function ensureRoomCatalogSubscription() {
  if (typeof window === "undefined") return
  bindWorkbenchCatalog()
  if (subscribed) return
  subscribed = true
  void subscribe<RoomChanged>(ROOM_CHANGED_EVENT, () => {
    void useRoomCatalogStore.getState().refresh()
  })
}
