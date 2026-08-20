import type {
  CollaborationRoomSummary,
  DbConversationSummary,
} from "@/lib/types"

/**
 * One selectable sidebar row. Sessions and Rooms share one multi-select, but
 * their id spaces differ (number vs string), so selection keys are prefixed
 * strings and the value carries the kind.
 */
export type SidebarSelectionItem =
  | { kind: "session"; session: DbConversationSummary }
  | { kind: "room"; room: CollaborationRoomSummary }

const SESSION_KEY_PREFIX = "session:"
const ROOM_KEY_PREFIX = "room:"

export function sessionItemKey(id: number): string {
  return `${SESSION_KEY_PREFIX}${id}`
}

export function roomItemKey(id: string): string {
  return `${ROOM_KEY_PREFIX}${id}`
}

export function itemKeyOf(item: SidebarSelectionItem): string {
  return item.kind === "session"
    ? sessionItemKey(item.session.id)
    : roomItemKey(item.room.id)
}

export function parseItemKey(
  key: string
): { kind: "session"; id: number } | { kind: "room"; id: string } | null {
  if (key.startsWith(SESSION_KEY_PREFIX)) {
    const id = Number(key.slice(SESSION_KEY_PREFIX.length))
    return Number.isInteger(id) ? { kind: "session", id } : null
  }
  if (key.startsWith(ROOM_KEY_PREFIX)) {
    return { kind: "room", id: key.slice(ROOM_KEY_PREFIX.length) }
  }
  return null
}

/** Sessions inside a mixed selection, in selection (insertion) order. */
export function selectionSessions(
  selected: ReadonlyMap<string, SidebarSelectionItem>
): DbConversationSummary[] {
  const result: DbConversationSummary[] = []
  for (const item of selected.values()) {
    if (item.kind === "session") result.push(item.session)
  }
  return result
}

/** Rooms inside a mixed selection, in selection (insertion) order. */
export function selectionRooms(
  selected: ReadonlyMap<string, SidebarSelectionItem>
): CollaborationRoomSummary[] {
  const result: CollaborationRoomSummary[] = []
  for (const item of selected.values()) {
    if (item.kind === "room") result.push(item.room)
  }
  return result
}
