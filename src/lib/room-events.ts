export const OPEN_ROOM_EVENT = "codeg:open-room"
export const ROOM_CHANGED_EVENT = "room://changed"

export function requestOpenRoom(roomId: string) {
  window.dispatchEvent(new CustomEvent(OPEN_ROOM_EVENT, { detail: { roomId } }))
}
