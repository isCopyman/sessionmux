"use client"

import { useCallback, useEffect, useMemo, useState } from "react"
import {
  Archive,
  ChevronDown,
  FolderTree,
  Loader2,
  PanelsTopLeft,
  Plus,
  Trash2,
  Users,
  X,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useTabActions } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import { toErrorMessage } from "@/lib/app-error"
import { formatConversationTitle } from "@/lib/conversation-title"
import {
  collectionsAllowedForRooms,
  deleteRooms,
  moveRoomsToCollection,
  openRoomsInCurrentWorkbench,
} from "@/lib/room-bulk-operations"
import {
  archiveSessions,
  deleteSessions,
  moveSessionsToCollection,
} from "@/lib/session-bulk-operations"
import { cn } from "@/lib/utils"
import {
  createCollaborationRoom,
  listConversationCollectionRefs,
} from "@/lib/api"
import { useOpenRoom } from "@/lib/open-room"
import {
  appendConversationsToWorkbench,
  appendRoomsToWorkbench,
  conversationIdsOccupiedElsewhereFor,
  SIDEBAR_BULK_TAB_ORIGIN,
} from "@/lib/workbench-session-tabs"
import type {
  CollaborationRoomSummary,
  CollectionInfo,
  DbConversationSummary,
} from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useCollectionStore } from "@/stores/collection-store"
import { useTabStore } from "@/stores/tab-store"
import { useWorkbenchStore } from "@/stores/workbench-store"

function collectionMenuOptions(items: CollectionInfo[]) {
  const children = new Map<number | null, CollectionInfo[]>()
  for (const item of items) {
    const siblings = children.get(item.parent_id) ?? []
    siblings.push(item)
    children.set(item.parent_id, siblings)
  }
  for (const siblings of children.values()) {
    siblings.sort((a, b) => a.position - b.position || a.id - b.id)
  }
  const result: Array<{ item: CollectionInfo; depth: number }> = []
  const seen = new Set<number>()
  const append = (item: CollectionInfo, depth: number) => {
    if (seen.has(item.id)) return
    seen.add(item.id)
    result.push({ item, depth })
    for (const child of children.get(item.id) ?? []) append(child, depth + 1)
  }
  for (const root of children.get(null) ?? []) append(root, 0)
  for (const item of items) if (!seen.has(item.id)) append(item, 0)
  return result
}

interface SessionBulkActionBarProps {
  selected: ReadonlyMap<number, DbConversationSummary>
  /** Rooms sharing the sidebar multi-select. Actions that make no sense for a
   * Room (archive, create-Room) grey out while any Room is selected; the rest
   * handle both kinds. */
  selectedRooms?: readonly CollaborationRoomSummary[]
  onClear: () => void
  className?: string
}

export function SessionBulkActionBar({
  selected,
  selectedRooms,
  onClear,
  className,
}: SessionBulkActionBarProps) {
  const t = useTranslations("Folder.sidebar.manageConversations")
  const tCommon = useTranslations("Folder.common")
  const tWorkbench = useTranslations("Folder.workbench")
  const conversations = useMemo(() => [...selected.values()], [selected])
  const rooms = useMemo(() => selectedRooms ?? [], [selectedRooms])
  const sessionCount = conversations.length
  const roomCount = rooms.length
  const selectedCount = sessionCount + roomCount
  const { closeConversationTab, closeTab, openTab, openRoomTab } =
    useTabActions()
  const { openConversations } = useWorkbenchRoute()
  const openRoom = useOpenRoom()
  const activeWorkbenchId = useTabStore((state) => state.activeWorkbenchId)
  const activeWorkbenchTabs = useTabStore((state) => state.rawTabs)
  const folders = useAppWorkspaceStore((state) => state.folders)
  const workbenches = useWorkbenchStore((state) => state.items)
  const workbenchesHydrated = useWorkbenchStore((state) => state.hydrated)
  const hydrateWorkbenches = useWorkbenchStore((state) => state.hydrate)
  const createOnly = useWorkbenchStore((state) => state.createOnly)
  const collections = useCollectionStore((state) => state.items)
  const collectionsHydrated = useCollectionStore((state) => state.hydrated)
  const hydrateCollections = useCollectionStore((state) => state.hydrate)
  const [pending, setPending] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)

  useEffect(() => {
    if (!workbenchesHydrated) {
      void hydrateWorkbenches().catch((error) => {
        toast.error(t("toastOpFailed", { message: toErrorMessage(error) }))
      })
    }
  }, [hydrateWorkbenches, t, workbenchesHydrated])

  useEffect(() => {
    if (!collectionsHydrated) {
      void hydrateCollections().catch((error) => {
        toast.error(t("toastOpFailed", { message: toErrorMessage(error) }))
      })
    }
  }, [collectionsHydrated, hydrateCollections, t])

  const collectionOptions = useMemo(
    () => collectionMenuOptions(collections),
    [collections]
  )
  // With Rooms in the selection a move target must sit on every Room's Path
  // root — the same restriction the tree's single-Room move menu applies.
  const moveTargets = useMemo(() => {
    if (roomCount === 0) return collectionOptions
    const allowed = new Set(
      collectionsAllowedForRooms(
        collectionOptions.map(({ item }) => item),
        rooms
      ).map((item) => item.id)
    )
    return collectionOptions.filter(({ item }) => allowed.has(item.id))
  }, [collectionOptions, roomCount, rooms])
  const activeWorkbenchName =
    workbenches.find((workbench) => workbench.id === activeWorkbenchId)?.name ??
    t("currentWorkbench")

  const run = useCallback(
    async (operation: () => Promise<void>) => {
      if (selectedCount === 0 || pending) return
      setPending(true)
      try {
        await operation()
        onClear()
      } catch (error) {
        toast.error(t("toastOpFailed", { message: toErrorMessage(error) }))
      } finally {
        setPending(false)
      }
    },
    [onClear, pending, selectedCount, t]
  )

  const handleArchive = useCallback(() => {
    void run(async () => {
      await archiveSessions(conversations)
      toast.success(t("toastArchived", { count: sessionCount }))
    })
  }, [conversations, run, sessionCount, t])

  const handleDelete = useCallback(() => {
    void run(async () => {
      if (conversations.length > 0) {
        await deleteSessions(conversations, closeConversationTab)
      }
      if (rooms.length > 0) {
        await deleteRooms(rooms, closeTab)
      }
      if (sessionCount > 0 && roomCount > 0) {
        toast.success(
          t("toastDeletedMixed", { sessions: sessionCount, rooms: roomCount })
        )
      } else if (roomCount > 0) {
        toast.success(t("toastRoomsDeleted", { count: roomCount }))
      } else {
        toast.success(t("toastDeleted", { count: sessionCount }))
      }
      setConfirmDelete(false)
    })
  }, [
    closeConversationTab,
    closeTab,
    conversations,
    roomCount,
    rooms,
    run,
    sessionCount,
    t,
  ])

  const handleMove = useCallback(
    (collectionId: number | null) => {
      void run(async () => {
        if (conversations.length > 0) {
          await moveSessionsToCollection(
            conversations.map((conversation) => conversation.id),
            collectionId
          )
        }
        if (rooms.length > 0) {
          await moveRoomsToCollection(rooms, collectionId)
        }
        if (sessionCount > 0 && roomCount > 0) {
          toast.success(
            t("toastMovedMixed", { sessions: sessionCount, rooms: roomCount })
          )
        } else if (roomCount > 0) {
          toast.success(t("toastRoomsMoved", { count: roomCount }))
        } else {
          toast.success(t("toastCollectionMoved", { count: sessionCount }))
        }
      })
    },
    [conversations, roomCount, rooms, run, sessionCount, t]
  )

  const handleAddToWorkbench = useCallback(
    async (target: "current" | "new" | number) => {
      if (selectedCount === 0 || pending) return
      setPending(true)
      try {
        let workbenchId = activeWorkbenchId
        let workbenchName = activeWorkbenchName
        if (target === "new") {
          const created = await createOnly(
            tWorkbench("defaultName", {
              number: workbenches.length + 1,
            })
          )
          workbenchId = created.id
          workbenchName = created.name
        } else if (target !== "current") {
          workbenchId = target
          workbenchName =
            workbenches.find((workbench) => workbench.id === target)?.name ??
            String(target)
        }

        if (workbenchId === activeWorkbenchId) {
          let sessionsAdded = 0
          let sessionsSkipped = 0
          if (conversations.length > 0) {
            const occupied = await conversationIdsOccupiedElsewhereFor(
              conversations.map((conversation) => conversation.id),
              [activeWorkbenchId]
            )
            const present = new Set(
              activeWorkbenchTabs
                .map((tab) => tab.conversationId)
                .filter((id): id is number => id != null)
            )
            const toAdd = conversations.filter(
              (conversation) =>
                !present.has(conversation.id) && !occupied.has(conversation.id)
            )
            sessionsSkipped = conversations.length - toAdd.length
            sessionsAdded = toAdd.length
            if (toAdd.length > 0) {
              openConversations()
              for (const conversation of toAdd) {
                openTab(
                  conversation.folder_id,
                  conversation.id,
                  conversation.agent_type,
                  true,
                  formatConversationTitle(conversation.title)
                )
              }
            }
            if (sessionsAdded === 0) {
              toast.success(
                t("toastAlreadyInWorkbench", { workbench: workbenchName })
              )
            } else {
              toast.success(
                sessionsSkipped > 0
                  ? t("toastAddedWithSkipped", {
                      added: sessionsAdded,
                      skipped: sessionsSkipped,
                      workbench: workbenchName,
                    })
                  : t("toastAddedToWorkbench", {
                      count: sessionsAdded,
                      workbench: workbenchName,
                    })
              )
            }
          }
          let roomsAdded = 0
          if (rooms.length > 0) {
            const openRoomIds = new Set(
              activeWorkbenchTabs
                .map((tab) => tab.roomId)
                .filter((id): id is string => id != null)
            )
            const result = openRoomsInCurrentWorkbench({
              rooms,
              openTabRoomIds: openRoomIds,
              openRoomTab,
              folders,
            })
            roomsAdded = result.added
            if (roomsAdded === 0) {
              toast.success(
                t("toastRoomsAlreadyInWorkbench", { workbench: workbenchName })
              )
            } else {
              if (sessionsAdded === 0) openConversations()
              toast.success(
                t("toastRoomsAddedToWorkbench", {
                  count: roomsAdded,
                  workbench: workbenchName,
                })
              )
            }
          }
          // Selection stays when nothing was added anywhere (mirrors the
          // Sessions-only "already in workbench" early exit).
          if (sessionsAdded + roomsAdded > 0) onClear()
          return
        }

        let sessionsAdded = 0
        if (conversations.length > 0) {
          const result = await appendConversationsToWorkbench(
            workbenchId,
            conversations,
            SIDEBAR_BULK_TAB_ORIGIN,
            { ignoreWorkbenchIds: [activeWorkbenchId] }
          )
          for (const id of result.addedIds) {
            const conversation = conversations.find((item) => item.id === id)
            if (conversation) {
              closeConversationTab(
                conversation.folder_id,
                conversation.id,
                conversation.agent_type
              )
            }
          }
          sessionsAdded = result.added
          toast.success(
            result.added === 0
              ? t("toastAlreadyInWorkbench", { workbench: workbenchName })
              : result.skipped > 0
                ? t("toastAddedWithSkipped", {
                    added: result.added,
                    skipped: result.skipped,
                    workbench: workbenchName,
                  })
                : t("toastAddedToWorkbench", {
                    count: result.added,
                    workbench: workbenchName,
                  })
          )
        }
        let roomsAdded = 0
        if (rooms.length > 0) {
          const result = await appendRoomsToWorkbench(
            workbenchId,
            rooms,
            folders
          )
          roomsAdded = result.added
          toast.success(
            result.added === 0
              ? t("toastRoomsAlreadyInWorkbench", { workbench: workbenchName })
              : t("toastRoomsAddedToWorkbench", {
                  count: result.added,
                  workbench: workbenchName,
                })
          )
        }
        if (sessionsAdded > 0 || roomsAdded > 0) onClear()
      } catch (error) {
        toast.error(t("toastOpFailed", { message: toErrorMessage(error) }))
      } finally {
        setPending(false)
      }
    },
    [
      activeWorkbenchId,
      activeWorkbenchName,
      activeWorkbenchTabs,
      closeConversationTab,
      conversations,
      createOnly,
      folders,
      onClear,
      openConversations,
      openRoomTab,
      openTab,
      pending,
      rooms,
      selectedCount,
      t,
      tWorkbench,
      workbenches,
    ]
  )

  const handleCreateRoom = useCallback(() => {
    // Only Sessions can be Room members; the button greys out while any Room
    // is selected, so this guard is reachable for Sessions-only selections.
    if (sessionCount < 2) {
      toast.error(t("toastRoomNeedTwo"))
      return
    }
    void run(async () => {
      const title =
        conversations
          .slice(0, 2)
          .map((conversation) => formatConversationTitle(conversation.title))
          .filter(Boolean)
          .join(" / ") || t("createRoom")
      const refs = await listConversationCollectionRefs(
        conversations.map((conversation) => conversation.id)
      )
      const collectionIds = new Set(refs.map((ref) => ref.collection_id))
      const collectionId =
        refs.length === conversations.length && collectionIds.size === 1
          ? refs[0]?.collection_id
          : undefined
      const created = await createCollaborationRoom({
        workbenchId: activeWorkbenchId,
        title: selectedCount > 2 ? `${title}…` : title,
        memberConversationIds: conversations.map(
          (conversation) => conversation.id
        ),
        createdByConversationId: conversations[0].id,
        ...(collectionId != null ? { collectionId } : {}),
      })
      toast.success(t("toastRoomCreated", { title: created.title }))
      await openRoom(created)
    })
  }, [activeWorkbenchId, conversations, openRoom, run, sessionCount, t])

  if (selectedCount === 0) return null

  return (
    <>
      <div
        role="region"
        aria-label={t("bulkActions")}
        data-session-bulk-bar=""
        className={cn(
          "shrink-0 border-t border-border/50 bg-sidebar px-1.5 py-1.5",
          className
        )}
      >
        <div className="mb-1 flex items-center gap-1 px-1">
          <span className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground">
            {t("selectedCount", { count: selectedCount })}
          </span>
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            className="h-6 w-6 text-muted-foreground"
            onClick={onClear}
            title={t("clearSelection")}
            aria-label={t("clearSelection")}
          >
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
        <div className="flex flex-wrap gap-1">
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-7 px-2 text-xs"
            // Rooms have no archive state — greyed out while any is selected.
            disabled={pending || roomCount > 0}
            onClick={handleArchive}
          >
            <Archive className="h-3.5 w-3.5" />
            {t("archiveSelected")}
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-7 px-2 text-xs"
                disabled={pending}
              >
                <FolderTree className="h-3.5 w-3.5" />
                {t("moveToCollection")}
                <ChevronDown className="h-3 w-3 opacity-60" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="start"
              className="max-h-72 w-56 overflow-y-auto"
            >
              <DropdownMenuItem onSelect={() => handleMove(null)}>
                <FolderTree className="h-3.5 w-3.5 opacity-50" />
                {t("collectionUnclassified")}
              </DropdownMenuItem>
              {moveTargets.map(({ item, depth }) => (
                <DropdownMenuItem
                  key={item.id}
                  onSelect={() => handleMove(item.id)}
                >
                  <FolderTree className="h-3.5 w-3.5" />
                  <span className="truncate">
                    {depth > 0 ? `${"· ".repeat(depth)}` : ""}
                    {item.name}
                  </span>
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-7 px-2 text-xs"
                disabled={pending}
              >
                {pending ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <PanelsTopLeft className="h-3.5 w-3.5" />
                )}
                {t("addToWorkbench")}
                <ChevronDown className="h-3 w-3 opacity-60" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="min-w-52">
              {workbenches.map((workbench) => (
                <DropdownMenuItem
                  key={workbench.id}
                  onSelect={() =>
                    void handleAddToWorkbench(
                      workbench.id === activeWorkbenchId
                        ? "current"
                        : workbench.id
                    )
                  }
                >
                  <PanelsTopLeft className="h-4 w-4" />
                  <span className="truncate">{workbench.name}</span>
                  {workbench.id === activeWorkbenchId ? (
                    <span className="opacity-60">
                      · {t("currentWorkbench")}
                    </span>
                  ) : null}
                </DropdownMenuItem>
              ))}
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onSelect={() => void handleAddToWorkbench("new")}
              >
                <Plus className="h-4 w-4" />
                {t("createNewWorkbench")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-7 px-2 text-xs"
            // Rooms cannot be Room members — greyed out while any is selected.
            disabled={pending || roomCount > 0}
            onClick={handleCreateRoom}
          >
            <Users className="h-3.5 w-3.5" />
            {t("createRoom")}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="destructive"
            className="h-7 px-2 text-xs"
            disabled={pending}
            onClick={() => setConfirmDelete(true)}
          >
            <Trash2 className="h-3.5 w-3.5" />
            {t("deleteSelected")}
          </Button>
        </div>
      </div>

      <AlertDialog
        open={confirmDelete}
        onOpenChange={(open) => !open && setConfirmDelete(false)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {sessionCount > 0 && roomCount > 0
                ? t("confirmDeleteMixedTitle", {
                    sessions: sessionCount,
                    rooms: roomCount,
                  })
                : roomCount > 0
                  ? t("confirmDeleteRoomsTitle", { count: roomCount })
                  : t("confirmDeleteTitle", { count: sessionCount })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t("confirmDeleteDescription")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{tCommon("cancel")}</AlertDialogCancel>
            <AlertDialogAction onClick={handleDelete}>
              {tCommon("confirm")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
