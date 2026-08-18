import type { CollaborationRoomSummary } from "@/lib/types"
import type { SidebarSortMode } from "@/lib/sidebar-view-mode-storage"

export function compareRoomsForSidebar(
  left: CollaborationRoomSummary,
  right: CollaborationRoomSummary,
  sortMode: SidebarSortMode
): number {
  const leftStamp =
    sortMode === "updated"
      ? (left.lastEventAt ?? left.updatedAt)
      : left.createdAt
  const rightStamp =
    sortMode === "updated"
      ? (right.lastEventAt ?? right.updatedAt)
      : right.createdAt
  return (
    Date.parse(rightStamp) - Date.parse(leftStamp) ||
    left.id.localeCompare(right.id)
  )
}

export function sortRoomsForSidebar(
  rooms: readonly CollaborationRoomSummary[],
  sortMode: SidebarSortMode
): CollaborationRoomSummary[] {
  return [...rooms].sort((left, right) =>
    compareRoomsForSidebar(left, right, sortMode)
  )
}
