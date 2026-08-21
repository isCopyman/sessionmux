"use client"

import { useMemo, useState } from "react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { ScrollArea } from "@/components/ui/scroll-area"
import { addCollaborationRoomMembers } from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import type { CollaborationRoomSummary } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useCollectionStore } from "@/stores/collection-store"
import { useRoomCatalogStore } from "@/stores/room-catalog-store"

/**
 * Session-side entry: pick an existing Room from the catalog and add this
 * Session as a member. Membership is not on the summary, so already-joined
 * Rooms stay listed; a duplicate join is rejected by the backend.
 */
export function JoinRoomDialog({
  open,
  onOpenChange,
  conversationId,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  conversationId: number
}) {
  const t = useTranslations("Room")
  const rooms = useRoomCatalogStore((state) => state.rooms)
  const collections = useCollectionStore((state) => state.items)
  const allFolders = useAppWorkspaceStore((state) => state.allFolders)
  const [query, setQuery] = useState("")
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [pending, setPending] = useState(false)

  const collectionById = useMemo(
    () => new Map(collections.map((item) => [item.id, item.name])),
    [collections]
  )
  const folderById = useMemo(
    () => new Map(allFolders.map((folder) => [folder.id, folder.name])),
    [allFolders]
  )

  const visibleRooms = useMemo(
    () => roomsMatchingQuery(rooms, query),
    [query, rooms]
  )

  const handleOpenChange = (next: boolean) => {
    onOpenChange(next)
    if (!next) {
      setQuery("")
      setSelectedId(null)
      setPending(false)
    }
  }

  const handleJoin = async () => {
    if (selectedId == null) return
    const room = rooms.find((item) => item.id === selectedId)
    setPending(true)
    try {
      await addCollaborationRoomMembers({
        roomId: selectedId,
        conversationIds: [conversationId],
      })
      toast.success(t("joinedToast", { title: room?.title ?? selectedId }))
      handleOpenChange(false)
      void useRoomCatalogStore.getState().refresh()
    } catch (error) {
      toast.error(toErrorMessage(error))
    } finally {
      setPending(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{t("joinRoomTitle")}</DialogTitle>
        </DialogHeader>
        <Input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t("joinRoomSearch")}
          aria-label={t("joinRoomSearch")}
        />
        <ScrollArea className="h-56">
          {visibleRooms.length === 0 ? (
            <p className="px-1 py-6 text-center text-xs text-muted-foreground">
              {t("joinRoomEmpty")}
            </p>
          ) : (
            <RadioGroup
              value={selectedId ?? ""}
              onValueChange={setSelectedId}
              className="flex flex-col gap-1"
            >
              {visibleRooms.map((room) => (
                <RoomPickRow
                  key={room.id}
                  room={room}
                  collectionName={
                    room.collectionId != null
                      ? collectionById.get(room.collectionId)
                      : undefined
                  }
                  folderName={
                    room.rootFolderId != null
                      ? folderById.get(room.rootFolderId)
                      : undefined
                  }
                  memberCountLabel={t("memberCount", {
                    count: room.memberCount,
                  })}
                />
              ))}
            </RadioGroup>
          )}
        </ScrollArea>
        <DialogFooter>
          <Button variant="outline" onClick={() => handleOpenChange(false)}>
            {t("cancel")}
          </Button>
          <Button
            disabled={selectedId == null || pending}
            onClick={() => void handleJoin()}
          >
            {t("joinRoomAction")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function roomsMatchingQuery(
  rooms: CollaborationRoomSummary[],
  query: string
): CollaborationRoomSummary[] {
  const needle = query.trim().toLowerCase()
  if (!needle) return rooms
  return rooms.filter((room) => room.title.toLowerCase().includes(needle))
}

function RoomPickRow({
  room,
  collectionName,
  folderName,
  memberCountLabel,
}: {
  room: CollaborationRoomSummary
  collectionName: string | undefined
  folderName: string | undefined
  memberCountLabel: string
}) {
  const context = [collectionName, folderName].filter(Boolean).join(" · ")
  return (
    <Label className="flex w-full cursor-pointer items-start gap-2 rounded-md px-2 py-1.5 text-left text-xs font-normal hover:bg-muted/60">
      <RadioGroupItem value={room.id} className="mt-0.5" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm">{room.title}</span>
        <span className="mt-0.5 block truncate text-[11px] text-muted-foreground">
          {memberCountLabel}
          {context ? ` · ${context}` : ""}
        </span>
      </span>
    </Label>
  )
}
