"use client"

import { useEffect, useMemo, useState } from "react"
import {
  ChevronDown,
  ChevronRight,
  Copy,
  Loader2,
  MoreHorizontal,
  PanelsTopLeft,
  Pencil,
  Pin,
  PinOff,
  Plus,
  Trash2,
  Users,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import { AgentIcon } from "@/components/agent-icon"
import { CollaborationUnreadBadge } from "@/components/collaboration/collaboration-unread-badge"
import { ConversationStatusDot } from "@/components/conversations/conversation-status-dot"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { useTabStore } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import { toErrorMessage } from "@/lib/app-error"
import { listOpenedTabs, listWorkbenchTabs } from "@/lib/api"
import { formatConversationTitle } from "@/lib/conversation-title"
import type {
  AgentType,
  CollaborationRoomSummary,
  ConversationStatus,
  DbConversationSummary,
  OpenedTab,
  WorkbenchInfo,
} from "@/lib/types"
import { useOpenRoom } from "@/lib/open-room"
import {
  ensureRoomCatalogSubscription,
  useRoomCatalogStore,
} from "@/stores/room-catalog-store"
import { cn } from "@/lib/utils"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useWorkbenchStore } from "@/stores/workbench-store"
import { useOrganizationRevisionStore } from "@/stores/organization-revision-store"

type EditorState =
  | { mode: "create" }
  | { mode: "rename"; item: WorkbenchInfo }
  | { mode: "duplicate"; item: WorkbenchInfo }

interface TreeSession {
  key: string
  kind: "conversation" | "room"
  roomId?: string
  room?: CollaborationRoomSummary
  conversationId: number | null
  folderId: number
  agentType: AgentType
  title: string
  status?: ConversationStatus
  liveTabId?: string
  updatedAt: string | null
  tabOrder: number
}

async function fetchWorkbenchTabs(workbenchId: number) {
  return workbenchId === 1 ? listOpenedTabs() : listWorkbenchTabs(workbenchId)
}

function persistedSessions(
  tabs: OpenedTab[],
  conversations: Map<number, DbConversationSummary>,
  untitled: string
): TreeSession[] {
  return sortTreeSessions(
    tabs.flatMap((tab, tabOrder) => {
      if (tab.conversation_id == null) return []
      const conversation = conversations.get(tab.conversation_id)
      return [
        {
          key: `conversation:${tab.conversation_id}`,
          kind: "conversation",
          conversationId: tab.conversation_id,
          folderId: conversation?.folder_id ?? tab.folder_id,
          agentType: conversation?.agent_type ?? tab.agent_type,
          title:
            formatConversationTitle(conversation?.title ?? null) || untitled,
          status: conversation?.status as ConversationStatus | undefined,
          updatedAt: conversation?.updated_at ?? null,
          tabOrder,
        },
      ]
    })
  )
}

/**
 * The Workbench tree is navigation, not a mirror of the pane strip. Dragging a
 * tab changes its visual slot only; the tree remains a useful activity list.
 * Drafts stay first, then persisted Sessions sort by newest update with the
 * saved tab position as a stable fallback when a summary is not loaded.
 */
function roomToTreeSession(
  room: CollaborationRoomSummary,
  conversations: Map<number, DbConversationSummary>,
  tabOrder: number,
  liveTabId?: string
): TreeSession {
  const creator = conversations.get(room.createdByConversationId)
  return {
    key: liveTabId ?? `room:${room.id}`,
    kind: "room",
    roomId: room.id,
    room,
    conversationId: null,
    folderId: room.rootFolderId ?? creator?.folder_id ?? 1,
    agentType: creator?.agent_type ?? "claude_code",
    title: room.title,
    liveTabId,
    updatedAt: room.lastEventAt ?? room.createdAt,
    tabOrder,
  }
}

export function sortTreeSessions(sessions: TreeSession[]): TreeSession[] {
  return [...sessions].sort((left, right) => {
    const leftDraft =
      left.kind === "conversation" && left.conversationId == null
    const rightDraft =
      right.kind === "conversation" && right.conversationId == null
    if (leftDraft !== rightDraft) return leftDraft ? -1 : 1
    const updatedDiff =
      Date.parse(right.updatedAt ?? "") - Date.parse(left.updatedAt ?? "")
    if (Number.isFinite(updatedDiff) && updatedDiff !== 0) return updatedDiff
    return left.tabOrder - right.tabOrder || left.key.localeCompare(right.key)
  })
}

/**
 * A visible tree of saved Workbenches and their Session references. Workbench
 * membership is intentionally independent from Collection ownership: the same
 * Session may appear under several Workbenches while retaining one semantic
 * home in the Collection tree below.
 */
export function WorkbenchTree() {
  const t = useTranslations("Folder.workbench")
  const tConversation = useTranslations("Folder.conversationCard")
  const items = useWorkbenchStore((state) => state.items)
  const hydrated = useWorkbenchStore((state) => state.hydrated)
  const loading = useWorkbenchStore((state) => state.loading)
  const hydrate = useWorkbenchStore((state) => state.hydrate)
  const createAndSwitch = useWorkbenchStore((state) => state.createAndSwitch)
  const duplicateAndSwitch = useWorkbenchStore(
    (state) => state.duplicateAndSwitch
  )
  const rename = useWorkbenchStore((state) => state.rename)
  const setPinned = useWorkbenchStore((state) => state.setPinned)
  const remove = useWorkbenchStore((state) => state.remove)

  const activeWorkbenchId = useTabStore((state) => state.activeWorkbenchId)
  const switching = useTabStore((state) => state.switchingWorkbench)
  const tabsHydrated = useTabStore((state) => state.tabsHydrated)
  const liveTabs = useTabStore((state) => state.tabs)
  const activeTabId = useTabStore((state) => state.activeTabId)
  const switchWorkbench = useTabStore((state) => state.switchWorkbench)
  const switchTab = useTabStore((state) => state.switchTab)
  const openTab = useTabStore((state) => state.openTab)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const { openConversations } = useWorkbenchRoute()
  const openRoom = useOpenRoom()
  const catalogRooms = useRoomCatalogStore((state) => state.rooms)
  const organizationRevision = useOrganizationRevisionStore(
    (state) => state.revision
  )

  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const [snapshots, setSnapshots] = useState<Map<number, OpenedTab[]>>(
    new Map()
  )
  const [snapshotsLoading, setSnapshotsLoading] = useState(false)
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [name, setName] = useState("")
  const [pending, setPending] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<WorkbenchInfo | null>(null)

  useEffect(() => {
    if (hydrated) return
    void hydrate().catch((error) =>
      toast.error(t("loadFailed", { message: toErrorMessage(error) }))
    )
  }, [hydrate, hydrated, t])

  useEffect(() => {
    if (!tabsHydrated) return
    setExpanded((current) => new Set(current).add(activeWorkbenchId))
  }, [activeWorkbenchId, tabsHydrated])

  const workbenchIdsKey = useMemo(
    () => items.map((item) => item.id).join(","),
    [items]
  )

  useEffect(() => {
    ensureRoomCatalogSubscription()
    void useRoomCatalogStore.getState().refresh()
  }, [workbenchIdsKey])

  useEffect(() => {
    if (!hydrated || items.length === 0) return
    let cancelled = false
    setSnapshotsLoading(true)
    Promise.all(
      items.map(
        async (item) =>
          [item.id, (await fetchWorkbenchTabs(item.id)).items] as const
      )
    )
      .then((entries) => {
        if (!cancelled) setSnapshots(new Map(entries))
      })
      .catch((error) => {
        if (!cancelled) {
          toast.error(t("loadFailed", { message: toErrorMessage(error) }))
        }
      })
      .finally(() => {
        if (!cancelled) setSnapshotsLoading(false)
      })
    return () => {
      cancelled = true
    }
    // Switching flushes the old Workbench and mounts a new one, so refresh the
    // inactive snapshots whenever the active identity changes.
  }, [
    activeWorkbenchId,
    hydrated,
    items,
    organizationRevision,
    t,
    workbenchIdsKey,
  ])

  const conversationById = useMemo(
    () =>
      new Map(
        conversations.map((conversation) => [conversation.id, conversation])
      ),
    [conversations]
  )

  const sessionsFor = (workbenchId: number): TreeSession[] => {
    const workbenchRooms = catalogRooms.filter(
      (room) => room.workbenchId === workbenchId
    )
    if (workbenchId === activeWorkbenchId) {
      const fromTabs = liveTabs.map((tab, tabOrder): TreeSession => {
        if (tab.kind === "room" && tab.roomId) {
          const room = workbenchRooms.find((item) => item.id === tab.roomId)
          return room
            ? roomToTreeSession(room, conversationById, tabOrder, tab.id)
            : {
                key: tab.id,
                kind: "room",
                roomId: tab.roomId,
                conversationId: null,
                folderId: tab.folderId,
                agentType: tab.agentType,
                title: tab.title,
                liveTabId: tab.id,
                updatedAt: null,
                tabOrder,
              }
        }
        const conversation =
          tab.conversationId == null
            ? undefined
            : conversationById.get(tab.conversationId)
        return {
          key: tab.id,
          kind: "conversation",
          conversationId: tab.conversationId,
          folderId: tab.folderId,
          agentType: tab.agentType,
          title: formatConversationTitle(tab.title) || t("draftSession"),
          status: tab.status,
          liveTabId: tab.id,
          updatedAt: conversation?.updated_at ?? null,
          tabOrder,
        }
      })
      const seen = new Set(
        fromTabs
          .filter((session) => session.kind === "room")
          .map((session) => session.roomId)
      )
      const extra = workbenchRooms
        .filter((room) => !seen.has(room.id))
        .map((room, index) =>
          roomToTreeSession(room, conversationById, fromTabs.length + index)
        )
      return sortTreeSessions([...fromTabs, ...extra])
    }
    return sortTreeSessions([
      ...persistedSessions(
        snapshots.get(workbenchId) ?? [],
        conversationById,
        t("untitledSession")
      ),
      ...workbenchRooms.map((room, index) =>
        roomToTreeSession(room, conversationById, 1000 + index)
      ),
    ])
  }

  const openEditor = (next: EditorState) => {
    setEditor(next)
    if (next.mode === "create") {
      setName(t("defaultName", { number: items.length + 1 }))
    } else if (next.mode === "duplicate") {
      setName(t("copyName", { name: next.item.name }))
    } else {
      setName(next.item.name)
    }
  }

  const submitEditor = async () => {
    if (!editor || pending || !name.trim()) return
    setPending(true)
    try {
      const normalized = name.trim()
      if (editor.mode === "create") {
        await createAndSwitch(normalized)
      } else if (editor.mode === "duplicate") {
        await duplicateAndSwitch(editor.item.id, normalized)
      } else {
        await rename(editor.item.id, normalized)
      }
      setEditor(null)
    } catch (error) {
      toast.error(t("saveFailed", { message: toErrorMessage(error) }))
    } finally {
      setPending(false)
    }
  }

  const confirmDelete = async () => {
    if (!deleteTarget || pending) return
    setPending(true)
    try {
      await remove(deleteTarget.id)
      setDeleteTarget(null)
    } catch (error) {
      toast.error(t("deleteFailed", { message: toErrorMessage(error) }))
    } finally {
      setPending(false)
    }
  }

  const togglePinned = async (item: WorkbenchInfo) => {
    try {
      await setPinned(item.id, !item.is_pinned)
    } catch (error) {
      toast.error(t("saveFailed", { message: toErrorMessage(error) }))
    }
  }

  const focusSession = async (workbenchId: number, session: TreeSession) => {
    try {
      if (session.kind === "room" && session.room) {
        await openRoom(session.room)
        return
      }
      if (workbenchId !== activeWorkbenchId) {
        await switchWorkbench(workbenchId)
      }
      openConversations()
      if (session.liveTabId && workbenchId === activeWorkbenchId) {
        switchTab(session.liveTabId)
      } else if (session.conversationId != null) {
        openTab(
          session.folderId,
          session.conversationId,
          session.agentType,
          true,
          session.title
        )
      }
    } catch (error) {
      toast.error(t("switchFailed", { message: toErrorMessage(error) }))
    }
  }

  return (
    <section className="shrink-0 border-b border-border/40 px-1.5 pb-1.5">
      <div className="flex h-7 items-center px-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
        <span className="min-w-0 flex-1 truncate">{t("treeTitle")}</span>
        <Button
          size="icon-sm"
          variant="ghost"
          className="h-6 w-6"
          aria-label={t("create")}
          onClick={() => openEditor({ mode: "create" })}
        >
          <Plus className="h-3.5 w-3.5" />
        </Button>
      </div>

      <div className="max-h-48 overflow-y-auto">
        {loading && items.length === 0 ? (
          <div className="flex h-8 items-center justify-center text-muted-foreground">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          </div>
        ) : (
          items.map((item) => {
            const active = item.id === activeWorkbenchId
            const isExpanded = expanded.has(item.id)
            const sessions = sessionsFor(item.id)
            return (
              <div key={item.id}>
                <div
                  className={cn(
                    "group flex h-7 min-w-0 items-center rounded-md pe-1 hover:bg-sidebar-accent",
                    active && "bg-sidebar-primary/8"
                  )}
                >
                  <button
                    type="button"
                    className="flex h-6 w-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground"
                    aria-label={isExpanded ? t("collapse") : t("expand")}
                    onClick={() =>
                      setExpanded((current) => {
                        const next = new Set(current)
                        if (next.has(item.id)) next.delete(item.id)
                        else next.add(item.id)
                        return next
                      })
                    }
                  >
                    {isExpanded ? (
                      <ChevronDown className="h-3.5 w-3.5" />
                    ) : (
                      <ChevronRight className="h-3.5 w-3.5 rtl:rotate-180" />
                    )}
                  </button>
                  <button
                    type="button"
                    className="flex h-full min-w-0 flex-1 items-center gap-1.5 text-start text-xs"
                    aria-current={active ? "page" : undefined}
                    aria-label={item.name}
                    title={item.name}
                    disabled={switching}
                    onClick={() => {
                      setExpanded((current) => new Set(current).add(item.id))
                      if (!active) {
                        void switchWorkbench(item.id).catch((error) =>
                          toast.error(
                            t("switchFailed", {
                              message: toErrorMessage(error),
                            })
                          )
                        )
                      }
                    }}
                  >
                    {active && switching ? (
                      <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-primary" />
                    ) : (
                      <PanelsTopLeft
                        className={cn(
                          "h-3.5 w-3.5 shrink-0 text-muted-foreground",
                          active && "text-primary"
                        )}
                      />
                    )}
                    <span className="truncate">{item.name}</span>
                    {item.is_pinned ? (
                      <Pin
                        aria-hidden
                        className="h-3 w-3 shrink-0 text-primary/75"
                      />
                    ) : null}
                    <span className="ms-auto shrink-0 text-[10px] text-muted-foreground">
                      {sessions.length}
                    </span>
                  </button>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        size="icon-sm"
                        variant="ghost"
                        className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
                        aria-label={t("actions", { name: item.name })}
                      >
                        <MoreHorizontal className="h-3.5 w-3.5" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem
                        onSelect={() => void togglePinned(item)}
                      >
                        {item.is_pinned ? (
                          <PinOff className="h-4 w-4" />
                        ) : (
                          <Pin className="h-4 w-4" />
                        )}
                        {item.is_pinned
                          ? tConversation("unpin")
                          : tConversation("pin")}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={() => openEditor({ mode: "duplicate", item })}
                      >
                        <Copy className="h-4 w-4" />
                        {t("duplicate")}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={() => openEditor({ mode: "rename", item })}
                      >
                        <Pencil className="h-4 w-4" />
                        {t("rename")}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        disabled={items.length <= 1}
                        variant="destructive"
                        onSelect={() => setDeleteTarget(item)}
                      >
                        <Trash2 className="h-4 w-4" />
                        {t("delete")}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>

                {isExpanded ? (
                  sessions.length > 0 ? (
                    sessions.map((session) => {
                      const selected =
                        active && session.liveTabId === activeTabId
                      return (
                        <button
                          key={session.key}
                          type="button"
                          data-workbench-id={item.id}
                          data-workbench-session
                          data-focused-session={selected ? "true" : undefined}
                          data-conversation-id={
                            session.conversationId ?? undefined
                          }
                          data-room-id={session.roomId}
                          title={session.title}
                          aria-current={selected ? "page" : undefined}
                          className={cn(
                            "flex h-7 w-full min-w-0 items-center gap-1.5 rounded-md pe-2 ps-8 text-start text-xs",
                            "hover:bg-sidebar-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                            selected &&
                              "bg-primary/8 text-primary ring-1 ring-inset ring-primary/30"
                          )}
                          onClick={() => void focusSession(item.id, session)}
                        >
                          <span
                            aria-hidden
                            className="relative flex h-3.5 w-3.5 shrink-0 items-center justify-center"
                          >
                            {session.kind === "room" ? (
                              <Users className="h-3 w-3" />
                            ) : (
                              <AgentIcon
                                agentType={session.agentType}
                                className="h-3 w-3"
                              />
                            )}
                            {session.status ? (
                              <ConversationStatusDot
                                status={session.status}
                                size="sm"
                                className="absolute -bottom-0.5 -right-0.5 ring-1 ring-sidebar"
                              />
                            ) : null}
                          </span>
                          <span className="min-w-0 flex-1 truncate">
                            {session.title}
                          </span>
                          {session.kind === "room" ? (
                            <CollaborationUnreadBadge
                              count={session.room?.unreadCount ?? 0}
                              className="ms-auto"
                            />
                          ) : null}
                        </button>
                      )
                    })
                  ) : (
                    <div className="flex h-7 items-center ps-8 text-xs text-muted-foreground">
                      {snapshotsLoading && !active
                        ? t("loading")
                        : t("emptySessions")}
                    </div>
                  )
                ) : null}
              </div>
            )
          })
        )}
      </div>

      <Dialog
        open={editor != null}
        onOpenChange={(open) => !open && setEditor(null)}
      >
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>
              {editor?.mode === "create"
                ? t("createTitle")
                : editor?.mode === "duplicate"
                  ? t("duplicate")
                  : t("renameTitle")}
            </DialogTitle>
          </DialogHeader>
          <Input
            autoFocus
            value={name}
            maxLength={80}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void submitEditor()
            }}
            placeholder={t("namePlaceholder")}
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditor(null)}>
              {t("cancel")}
            </Button>
            <Button
              disabled={!name.trim() || pending}
              onClick={() => void submitEditor()}
            >
              {pending ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
              {t("save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={deleteTarget != null}
        onOpenChange={(open) => !open && setDeleteTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("deleteTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("deleteDescription", { name: deleteTarget?.name ?? "" })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={pending}
              onClick={(event) => {
                event.preventDefault()
                void confirmDelete()
              }}
            >
              {t("delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  )
}
