"use client"

import { useEffect } from "react"
import { useTranslations } from "next-intl"

import { useOpenRoom } from "@/lib/open-room"
import type { CollaborationRoomSummary } from "@/lib/types"
import {
  ensureRoomCatalogSubscription,
  useRoomCatalogStore,
} from "@/stores/room-catalog-store"

/** Longest Room title the chip shows inline. The full one stays in the
 *  tooltip, next to the raw id. */
export const ROOM_CHIP_NAME_MAX = 16

/** A transcript can hold dozens of Room letters, so the catalog's first load is
 *  kicked once per app run instead of once per card. Everything after that
 *  arrives on `ROOM_CHANGED_EVENT` / Workbench changes — the catalog never
 *  polls, and neither does this chip. */
let initialLoadRequested = false

function useCatalogRoom(roomId: string): CollaborationRoomSummary | null {
  const rooms = useRoomCatalogStore((state) => state.rooms)
  useEffect(() => {
    ensureRoomCatalogSubscription()
    if (initialLoadRequested || useRoomCatalogStore.getState().hydrated) return
    initialLoadRequested = true
    void useRoomCatalogStore.getState().refresh()
  }, [])
  if (!roomId) return null
  return rooms.find((room) => room.id === roomId) ?? null
}

/** `rm_<uuid>` shortened to something a human can still compare by eye. */
export function shortRoomId(roomId: string): string {
  const separator = roomId.indexOf("_")
  if (separator < 0) return roomId.length > 10 ? roomId.slice(0, 10) : roomId
  const tail = roomId.slice(separator + 1)
  if (tail.length <= 8) return roomId
  return `${roomId.slice(0, separator + 1)}${tail.slice(0, 8)}`
}

function chipName(title: string): string {
  const chars = [...title]
  if (chars.length <= ROOM_CHIP_NAME_MAX) return title
  return `${chars.slice(0, ROOM_CHIP_NAME_MAX).join("")}…`
}

const PILL_CLASS =
  "rounded bg-violet-500/15 px-1.5 py-0.5 text-[10px] font-medium text-violet-800 dark:text-violet-300"

/**
 * The Room channel marker on a Session-letter card.
 *
 * Resolved against the same event-driven Room catalog the sidebar reads, so the
 * card carries the Room's title and opens it the way every other Room entry
 * point does (`useOpenRoom`: switch Workbench if needed, pin the tab).
 *
 * A Room the catalog does not know — deleted, or a shadow Session imported from
 * another install's database — degrades to the truncated raw id and stays
 * inert. A dead link would be worse than an honest id.
 */
export function SessionRoomChip({ roomId }: { roomId?: string | null }) {
  const t = useTranslations("Collaboration")
  const id = roomId ?? ""
  const room = useCatalogRoom(id)

  if (room) return <ResolvedRoomChip room={room} roomId={id} />

  return (
    <span
      className="inline-flex min-w-0 max-w-full items-center gap-1"
      data-room-chip={id || undefined}
      data-room-resolved="false"
      title={id ? t("roomUnavailableHint", { roomId: id }) : undefined}
    >
      <span className={PILL_CLASS}>{t("channelRoom")}</span>
      {id ? (
        <span className="truncate font-mono text-[10px]">
          {shortRoomId(id)}
        </span>
      ) : null}
    </span>
  )
}

/** Split out so `useOpenRoom` — which needs the Workbench route context — is
 *  only mounted for Rooms this install can actually open. */
function ResolvedRoomChip({
  room,
  roomId,
}: {
  room: CollaborationRoomSummary
  roomId: string
}) {
  const t = useTranslations("Collaboration")
  const openRoom = useOpenRoom()
  const fullTitle = room.title.trim() || shortRoomId(roomId)

  return (
    <button
      type="button"
      className="inline-flex min-w-0 max-w-full appearance-none items-center gap-1 text-left hover:text-foreground hover:underline"
      data-room-chip={roomId}
      data-room-resolved="true"
      title={`${fullTitle} · ${roomId}`}
      aria-label={t("openRoomAria", { title: fullTitle })}
      onClick={() => {
        void openRoom(room)
      }}
    >
      <span className={PILL_CLASS}>{t("channelRoom")}</span>
      <span className="truncate">{chipName(fullTitle)}</span>
    </button>
  )
}
