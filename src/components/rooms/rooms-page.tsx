"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  EllipsisVertical,
  FolderPlus,
  MessagesSquare,
  Pencil,
  Plus,
  Reply,
  Send,
  Trash2,
  UserPlus,
  UserRound,
  Users,
  X,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import { useShallow } from "zustand/react/shallow"

import { AgentIcon } from "@/components/agent-icon"
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
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  RichComposer,
  type RichComposerHandle,
} from "@/components/chat/composer/rich-composer"
import { useComposerMentionLabels } from "@/components/chat/composer/use-composer-mention-labels"
import type { ReferenceSearch } from "@/components/chat/composer/suggestion/types"
import type { ReferenceKind } from "@/components/chat/composer/types"
import { useReferenceSearch } from "@/components/chat/composer/use-reference-search"
import { useTabActions } from "@/contexts/tab-context"
import { toErrorMessage } from "@/lib/app-error"
import { formatConversationTitle } from "@/lib/conversation-title"
import {
  addCollaborationRoomMembers,
  addCollaborationRoomPath,
  deleteCollaborationRoom,
  getCollaborationRoom,
  getCollaborationRoomTimeline,
  markCollaborationRoomSeen,
  postCollaborationRoomMessage,
  removeCollaborationRoomMember,
  removeCollaborationRoomPath,
  renameCollaborationRoom,
} from "@/lib/api"
import { subscribe } from "@/lib/platform"
import { ROOM_CHANGED_EVENT } from "@/lib/room-events"
import { cn } from "@/lib/utils"
import {
  mentionsHumanFromText,
  sessionIdsFromText,
} from "@/lib/collaboration-session-mentions"
import {
  mentionAllFromText,
  roomMessageMarkdown,
  sessionIdsFromAtAliases,
} from "@/lib/room-message-body"
import { buildRoomMentionSearch } from "@/components/rooms/room-mention-search"
import { RoomPostBody } from "@/components/rooms/room-post-body"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useCollectionStore } from "@/stores/collection-store"
import { useConversationRuntimeStore } from "@/stores/conversation-runtime-store"
import { useRoomCatalogStore } from "@/stores/room-catalog-store"
import { makeRoomTabId } from "@/stores/tab-store"
import type {
  AgentType,
  CollaborationRoomDetail,
  CollaborationRoomMember,
  DbConversationSummary,
  RoomChanged,
  RoomTimelineEvent,
} from "@/lib/types"

function memberLabel(
  member: Pick<CollaborationRoomMember, "conversationId" | "title">,
  untitled: (id: number) => string
) {
  const title = member.title?.trim()
  return title
    ? formatConversationTitle(title)
    : untitled(member.conversationId)
}

function conversationLabel(
  conversation: DbConversationSummary,
  untitled: (id: number) => string
) {
  return (
    formatConversationTitle(conversation.title) || untitled(conversation.id)
  )
}

const GROUP_MS = 5 * 60 * 1000

function formatRoomTime(iso: string) {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ""
  return date.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  })
}

function sameSpeaker(a: RoomTimelineEvent, b: RoomTimelineEvent) {
  if ((a.authorKind ?? "session") !== (b.authorKind ?? "session")) return false
  if (a.authorKind === "human") return true
  return a.source.conversationId === b.source.conversationId
}

function RoomReplyButton({
  label,
  hint,
  compact = false,
  onReply,
}: {
  label: string
  hint: string
  compact?: boolean
  onReply: () => void
}) {
  return (
    <Button
      type="button"
      size="sm"
      variant="ghost"
      className={cn(
        "h-6 shrink-0 text-[11px] text-muted-foreground opacity-0 group-hover:opacity-100 group-focus-within:opacity-100",
        compact ? "w-6 px-0" : "px-1.5"
      )}
      title={hint}
      aria-label={label}
      onClick={onReply}
    >
      <Reply className="size-3" />
      {compact ? null : label}
    </Button>
  )
}

function RoomSpeakerAvatar({
  human,
  agentType,
  size = "md",
}: {
  human: boolean
  agentType?: string | null
  size?: "sm" | "md"
}) {
  return (
    <span
      className={cn(
        "flex shrink-0 items-center justify-center overflow-hidden rounded-full ring-1 ring-black/5 dark:ring-white/10",
        size === "md" ? "size-9" : "size-7",
        human ? "bg-primary/15 text-primary" : "bg-muted"
      )}
    >
      {human ? (
        <UserRound className={size === "md" ? "size-4" : "size-3.5"} />
      ) : (
        <AgentIcon
          agentType={(agentType ?? "claude_code") as AgentType}
          className={size === "md" ? "size-[18px]" : "size-3.5"}
        />
      )}
    </span>
  )
}

// The Room `@` panel never offers an agent tab (agents have no wake semantics
// inside a room — see `room-mention-search.ts`); session (the member roster)
// stays first so the panel's default-active tab still lands on members.
const ROOM_MENTION_TAB_ORDER: readonly ReferenceKind[] = [
  "session",
  "file",
  "commit",
]

export function RoomWorkspace({ roomId }: { roomId: string }) {
  const t = useTranslations("Room")
  const { openTab, closeTab } = useTabActions()
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const allFolders = useAppWorkspaceStore((state) => state.allFolders)
  const collections = useCollectionStore((state) => state.items)
  const [detail, setDetail] = useState<CollaborationRoomDetail | null>(null)
  const [events, setEvents] = useState<RoomTimelineEvent[]>([])
  const [truncated, setTruncated] = useState(false)
  const [loadingOlder, setLoadingOlder] = useState(false)
  const [replyTo, setReplyTo] = useState<RoomTimelineEvent | null>(null)
  const [hydrated, setHydrated] = useState(false)
  // The composer is an uncontrolled RichComposer; `body` is its serialized
  // plain-text mirror (mention badges serialize to `[label](codeg://…)` link
  // tokens), which drives the send gate, the wake preview and the post parse.
  const [body, setBody] = useState("")
  const [pending, setPending] = useState(false)
  const [titleDraft, setTitleDraft] = useState<string | null>(null)
  const [addOpen, setAddOpen] = useState(false)
  const [addQuery, setAddQuery] = useState("")
  const [addSelected, setAddSelected] = useState<number[]>([])
  const [adding, setAdding] = useState(false)
  const [pathsOpen, setPathsOpen] = useState(false)
  const [newPath, setNewPath] = useState("")
  const [addingPath, setAddingPath] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [membersOpen, setMembersOpen] = useState(false)
  const [removeTarget, setRemoveTarget] =
    useState<CollaborationRoomMember | null>(null)
  const [removingMember, setRemovingMember] = useState(false)
  const composerRef = useRef<RichComposerHandle>(null)
  // Localized chrome for the shared `@` panel (same hook the Session composer
  // uses — the panel reads identically wherever it opens).
  const { groupLabels: mentionGroupLabels, uiLabels: mentionUiLabels } =
    useComposerMentionLabels()

  const reload = useCallback(async () => {
    const [nextDetail, timeline] = await Promise.all([
      getCollaborationRoom(roomId),
      getCollaborationRoomTimeline(roomId),
    ])
    setDetail(nextDetail)
    setEvents(timeline.events)
    setTruncated(Boolean(timeline.truncated))
    setHydrated(true)
    void markCollaborationRoomSeen(roomId).catch(() => undefined)
  }, [roomId])

  const loadOlder = useCallback(async () => {
    const firstId = events[0]?.id
    if (!firstId || loadingOlder) return
    setLoadingOlder(true)
    try {
      const older = await getCollaborationRoomTimeline(
        roomId,
        undefined,
        firstId
      )
      setEvents((current) => {
        const seen = new Set(current.map((event) => event.id))
        return [
          ...older.events.filter((event) => !seen.has(event.id)),
          ...current,
        ]
      })
      setTruncated(Boolean(older.truncated))
    } catch (error) {
      toast.error(toErrorMessage(error))
    } finally {
      setLoadingOlder(false)
    }
  }, [events, loadingOlder, roomId])

  useEffect(() => {
    setReplyTo(null)
  }, [roomId])

  useEffect(() => {
    void reload().catch((error) => {
      toast.error(toErrorMessage(error))
      setHydrated(true)
    })
  }, [reload])

  useEffect(() => {
    let disposed = false
    let unsubscribe: (() => void) | undefined
    void subscribe<RoomChanged>(ROOM_CHANGED_EVENT, (change) => {
      if (disposed || change.roomId !== roomId) return
      void reload().catch(() => undefined)
      void useRoomCatalogStore.getState().refresh()
    }).then((off) => {
      if (disposed) off()
      else unsubscribe = off
    })
    return () => {
      disposed = true
      unsubscribe?.()
    }
  }, [reload, roomId])

  const eventsById = useMemo(
    () => new Map(events.map((item) => [item.id, item])),
    [events]
  )
  const members = useMemo(() => detail?.members ?? [], [detail])
  const memberIds = useMemo(
    () => new Set(members.map((member) => member.conversationId)),
    [members]
  )
  const runningMembers = useConversationRuntimeStore(
    useShallow((state) => {
      const running: Record<number, true> = {}
      for (const id of memberIds) {
        if (state.byConversationId.get(id)?.liveMessage != null) {
          running[id] = true
        }
      }
      return running
    })
  )
  const canPost = Boolean(body.trim()) && !pending && hydrated
  const speakerName = useCallback(
    (event: RoomTimelineEvent) => {
      if (event.authorKind === "human") return t("you")
      return memberLabel(
        {
          conversationId: event.source.conversationId,
          title: event.source.title,
        },
        (id) => t("untitled", { id })
      )
    },
    [t]
  )
  const openSession = useCallback(
    (conversationId: number) => {
      const live = conversations.find(
        (conversation) => conversation.id === conversationId
      )
      if (!live) return
      openTab(
        live.folder_id,
        live.id,
        live.agent_type as AgentType,
        true,
        formatConversationTitle(live.title) || undefined
      )
    },
    [conversations, openTab]
  )
  const scrollToEvent = useCallback((eventId: string) => {
    document.getElementById(`room-event-${eventId}`)?.scrollIntoView({
      block: "center",
      behavior: "smooth",
    })
  }, [])
  // Files/commits scoped to the room's own folder (resolved from
  // `rootFolderId`, never the currently-active folder — a room is a
  // cross-folder concept, so guessing wrong is worse than showing nothing).
  const roomFolderPath = useMemo(() => {
    const rootFolderId = detail?.rootFolderId
    if (rootFolderId == null) return null
    return allFolders.find((folder) => folder.id === rootFolderId)?.path ?? null
  }, [allFolders, detail?.rootFolderId])
  // Manually-added extra `@`-search roots, on top of `roomFolderPath` — see
  // the "manage paths" dialog below. Memoized so an unchanged additional-path
  // list keeps the same array identity across renders (`useFileTree` derives
  // its own cache key from the values, not this identity, but there is no
  // reason to churn it either).
  const additionalPaths = useMemo(
    () => detail?.additionalPaths?.map((entry) => entry.path) ?? [],
    [detail?.additionalPaths]
  )
  const workspaceReferenceSearch = useReferenceSearch({
    defaultPath: roomFolderPath,
    additionalPaths,
    enabled: true,
    labels: mentionGroupLabels,
  })
  // The `@` panel: the unified composer suggestion popup, scoped to this
  // room — members plus the two structured pseudo-mentions (@all / @human)
  // always lead, folded together with the file/commit groups from the room's
  // folder (empty when it can't be resolved). Every session row inserts a
  // reference badge that serializes to a `codeg://` URI token, so a wake is
  // ALWAYS structured (never typed prose).
  const roomMentionSearch = useMemo<ReferenceSearch>(
    () =>
      buildRoomMentionSearch(
        members,
        {
          sessionGroupLabel: mentionGroupLabels.session,
          allLabel: t("mentionAll"),
          humanLabel: t("mentionHuman"),
          untitled: (id) => t("untitled", { id }),
        },
        workspaceReferenceSearch
      ),
    [members, mentionGroupLabels, t, workspaceReferenceSearch]
  )

  // Live "will wake" preview: exactly the parse handlePost performs, so what
  // the host sees is what gets woken — no more silent at-submit resolution.
  const wakePreview = useMemo(() => {
    const untitled = (id: number) => t("untitled", { id })
    const ids = [
      ...new Set([
        ...sessionIdsFromText(body),
        ...sessionIdsFromAtAliases(body, members, untitled),
      ]),
    ]
    const names = ids.map((id) => {
      const member = members.find((item) => item.conversationId === id)
      return member ? memberLabel(member, untitled) : untitled(id)
    })
    if (mentionAllFromText(body)) names.unshift(t("mentionAll"))
    if (mentionsHumanFromText(body)) names.push(t("mentionHuman"))
    return names
  }, [body, members, t])

  const candidates = useMemo(() => {
    const query = addQuery.trim().toLowerCase()
    return conversations
      .filter(
        (conversation) =>
          conversation.archived_at == null && !memberIds.has(conversation.id)
      )
      .filter((conversation) => {
        if (!query) return true
        const title = formatConversationTitle(conversation.title).toLowerCase()
        return title.includes(query) || String(conversation.id).includes(query)
      })
  }, [addQuery, conversations, memberIds])

  const handlePost = useCallback(async () => {
    if (!detail || !body.trim()) return
    const text = body.trim()
    const untitled = (id: number) => t("untitled", { id })
    const mentionedFromUri = sessionIdsFromText(text)
    const mentionedFromAlias = sessionIdsFromAtAliases(
      text,
      detail.members,
      untitled
    )
    const mentionAllNext = mentionAllFromText(text)
    const targets = mentionAllNext
      ? []
      : [...new Set([...mentionedFromUri, ...mentionedFromAlias])]
    const mentionHumanNext = mentionsHumanFromText(text)
    setPending(true)
    try {
      await postCollaborationRoomMessage({
        roomId,
        sourceConversationId: detail.createdByConversationId,
        targetConversationIds: targets,
        mentionAll: mentionAllNext,
        mentionHuman: mentionHumanNext,
        authorKind: "human",
        body: text,
        clientDedupeId: crypto.randomUUID(),
        invocationPolicy:
          mentionAllNext || targets.length > 0
            ? "invoke_when_idle"
            : "store_only",
        expectsReply: mentionAllNext || targets.length > 0,
        replyToEventId: replyTo?.id ?? null,
      })
      composerRef.current?.clear()
      setBody("")
      setReplyTo(null)
      toast.success(t("posted"))
      await reload()
      void useRoomCatalogStore.getState().refresh()
    } catch (error) {
      toast.error(t("sendFailed"))
      console.error(toErrorMessage(error))
    } finally {
      setPending(false)
    }
  }, [body, detail, reload, replyTo, roomId, t])

  const handleRename = useCallback(async () => {
    if (!detail || titleDraft == null) return
    const next = titleDraft.trim()
    if (!next || next === detail.title) {
      setTitleDraft(null)
      return
    }
    try {
      const updated = await renameCollaborationRoom(roomId, next)
      setDetail(updated)
      setTitleDraft(null)
      toast.success(t("renamed"))
      void useRoomCatalogStore.getState().refresh()
    } catch (error) {
      toast.error(toErrorMessage(error))
    }
  }, [detail, roomId, t, titleDraft])

  const handleAddMembers = useCallback(async () => {
    if (addSelected.length === 0) return
    setAdding(true)
    try {
      const updated = await addCollaborationRoomMembers({
        roomId,
        conversationIds: addSelected,
      })
      setDetail(updated)
      setAddOpen(false)
      setAddSelected([])
      setAddQuery("")
      toast.success(t("added"))
      void useRoomCatalogStore.getState().refresh()
    } catch (error) {
      toast.error(toErrorMessage(error))
    } finally {
      setAdding(false)
    }
  }, [addSelected, roomId, t])

  const handleRemove = useCallback(
    async (conversationId: number) => {
      setRemovingMember(true)
      try {
        const updated = await removeCollaborationRoomMember(
          roomId,
          conversationId
        )
        setDetail(updated)
        setRemoveTarget(null)
        toast.success(t("removed"))
        void useRoomCatalogStore.getState().refresh()
      } catch (error) {
        toast.error(toErrorMessage(error))
      } finally {
        setRemovingMember(false)
      }
    },
    [roomId, t]
  )

  const handleAddPath = useCallback(async () => {
    const path = newPath.trim()
    if (!path) return
    setAddingPath(true)
    try {
      const updated = await addCollaborationRoomPath(roomId, path)
      setDetail(updated)
      setNewPath("")
      toast.success(t("pathAdded"))
    } catch (error) {
      toast.error(toErrorMessage(error))
    } finally {
      setAddingPath(false)
    }
  }, [newPath, roomId, t])

  const handleRemovePath = useCallback(
    async (pathId: number) => {
      try {
        const updated = await removeCollaborationRoomPath(roomId, pathId)
        setDetail(updated)
        toast.success(t("pathRemoved"))
      } catch (error) {
        toast.error(toErrorMessage(error))
      }
    },
    [roomId, t]
  )

  const handleDelete = useCallback(async () => {
    setDeleting(true)
    try {
      await deleteCollaborationRoom(roomId)
      setDeleteOpen(false)
      toast.success(t("deleted"))
      closeTab(makeRoomTabId(roomId))
      void useRoomCatalogStore.getState().refresh()
    } catch (error) {
      toast.error(toErrorMessage(error))
    } finally {
      setDeleting(false)
    }
  }, [closeTab, roomId, t])

  if (!hydrated) {
    return <div className="h-full" />
  }

  if (!detail) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 px-8 text-center text-sm text-muted-foreground">
        <MessagesSquare className="h-6 w-6 opacity-50" />
        <p>{t("emptyHint")}</p>
      </div>
    )
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-10 items-center justify-between gap-3 border-b border-border/60 px-4">
        <div className="min-w-0">
          {titleDraft == null ? (
            <button
              type="button"
              className="flex min-w-0 items-center gap-2 text-left"
              onClick={() => setTitleDraft(detail.title)}
              title={t("rename")}
            >
              <span className="flex size-6 items-center justify-center rounded-md bg-primary/10 text-primary">
                <MessagesSquare className="size-3.5" />
              </span>
              <h2 className="truncate text-[15px] font-semibold">
                {detail.title}
              </h2>
            </button>
          ) : (
            <Input
              autoFocus
              value={titleDraft}
              maxLength={80}
              className="h-8"
              onChange={(event) => setTitleDraft(event.target.value)}
              onBlur={() => void handleRename()}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault()
                  void handleRename()
                }
                if (event.key === "Escape") setTitleDraft(null)
              }}
            />
          )}
          <p className="mt-1 ps-8 truncate text-[11px] text-muted-foreground">
            {(detail.collectionId != null
              ? collections.find(
                  (collection) => collection.id === detail.collectionId
                )?.name
              : null) ?? t("uncategorized")}
            {" · "}
            <button
              type="button"
              className="rounded-xs hover:text-foreground hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              title={membersOpen ? t("hideMembers") : t("showMembers")}
              aria-pressed={membersOpen}
              onClick={() => setMembersOpen((open) => !open)}
            >
              {t("memberCount", { count: detail.members.length })}
            </button>
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-0.5">
          <Button
            type="button"
            size="icon-sm"
            variant={membersOpen ? "secondary" : "ghost"}
            className="h-7 w-7 text-muted-foreground"
            aria-label={membersOpen ? t("hideMembers") : t("showMembers")}
            aria-pressed={membersOpen}
            onClick={() => setMembersOpen((open) => !open)}
          >
            <Users className="h-3.5 w-3.5" />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                className="h-7 w-7 text-muted-foreground"
                aria-label={t("moreActions")}
                title={t("moreActions")}
              >
                <EllipsisVertical className="h-3.5 w-3.5" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={() => setTitleDraft(detail.title)}>
                <Pencil className="h-4 w-4" />
                {t("rename")}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => setAddOpen(true)}>
                <UserPlus className="h-4 w-4" />
                {t("addMember")}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => setPathsOpen(true)}>
                <FolderPlus className="h-4 w-4" />
                {t("managePaths")}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                variant="destructive"
                onSelect={() => setDeleteOpen(true)}
              >
                <Trash2 className="h-4 w-4" />
                {t("delete")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      <div className="flex min-h-0 flex-1">
        <ScrollArea className="min-w-0 flex-1">
          <div className="mx-auto flex w-full max-w-3xl flex-col py-2">
            {truncated ? (
              <div className="flex justify-center py-2">
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  className="h-7 px-2 text-xs text-muted-foreground"
                  disabled={loadingOlder}
                  onClick={() => void loadOlder()}
                >
                  {t("loadOlder")}
                </Button>
              </div>
            ) : null}
            {events.length === 0 ? (
              <p className="px-4 py-8 text-sm text-muted-foreground">
                {t("timelineEmpty")}
              </p>
            ) : (
              events.map((event, index) => {
                const fromYou = event.authorKind === "human"
                const quoted = event.replyToEventId
                  ? eventsById.get(event.replyToEventId)
                  : undefined
                const previous = events[index - 1]
                const grouped = Boolean(
                  previous &&
                  sameSpeaker(previous, event) &&
                  Math.abs(
                    new Date(event.createdAt).getTime() -
                      new Date(previous.createdAt).getTime()
                  ) < GROUP_MS
                )
                const markdown = roomMessageMarkdown({
                  body: event.body,
                  members: detail.members,
                  mentionConversationIds: event.mentionConversationIds,
                  mentionHuman: Boolean(event.mentionHuman),
                  allLabel: t("mentionAll"),
                  humanLabel: t("mentionHuman"),
                  untitled: (id) => t("untitled", { id }),
                })
                const name = fromYou
                  ? t("you")
                  : memberLabel(
                      {
                        conversationId: event.source.conversationId,
                        title: event.source.title,
                      },
                      (id) => t("untitled", { id })
                    )
                return (
                  <article
                    key={event.id}
                    id={`room-event-${event.id}`}
                    data-mention-human={event.mentionHuman ? "true" : undefined}
                    className={cn(
                      "group flex gap-3 border-l-2 px-4 hover:bg-muted/40",
                      grouped ? "py-0.5" : "mt-2 py-1.5",
                      event.mentionHuman
                        ? "border-amber-500 bg-amber-500/10"
                        : "border-transparent"
                    )}
                  >
                    {grouped ? (
                      <span className="flex w-9 shrink-0 items-start justify-center pt-0.5">
                        <RoomReplyButton
                          label={t("reply")}
                          hint={t("replyHint")}
                          compact
                          onReply={() => setReplyTo(event)}
                        />
                      </span>
                    ) : (
                      <RoomSpeakerAvatar
                        human={fromYou}
                        agentType={event.source.agentType}
                      />
                    )}
                    <div className="min-w-0 flex-1">
                      {grouped ? null : (
                        <div className="mb-0.5 flex items-baseline gap-2">
                          {fromYou ? (
                            <span className="truncate text-[15px] font-bold leading-5">
                              {name}
                            </span>
                          ) : (
                            <button
                              type="button"
                              className="truncate text-[15px] font-bold leading-5 hover:underline"
                              title={t("openSession")}
                              onClick={() =>
                                openSession(event.source.conversationId)
                              }
                            >
                              {name}
                            </button>
                          )}
                          <time className="shrink-0 text-[11px] text-muted-foreground">
                            {formatRoomTime(event.createdAt)}
                          </time>
                          {event.expectsReply ? (
                            <span className="shrink-0 rounded-full bg-amber-500/15 px-1.5 py-px text-[10px] font-medium text-amber-800 dark:text-amber-200">
                              {t("needsReplyBadge")}
                            </span>
                          ) : null}
                          <RoomReplyButton
                            label={t("reply")}
                            hint={t("replyHint")}
                            onReply={() => setReplyTo(event)}
                          />
                        </div>
                      )}
                      {event.replyToEventId ? (
                        quoted ? (
                          <button
                            type="button"
                            className="mb-1 w-full border-l-2 border-primary/40 py-0.5 pl-2 text-left"
                            title={t("jumpToQuote")}
                            onClick={() =>
                              scrollToEvent(event.replyToEventId as string)
                            }
                          >
                            <p className="truncate text-[11px] font-medium text-muted-foreground">
                              {speakerName(quoted)}
                            </p>
                            {/* Raw body on purpose: the quote is a two-line
                                locator for the post it points at, not a second
                                copy of it. Markdown here would grow headings
                                and code blocks inside a clamped preview. */}
                            <p className="line-clamp-2 text-[12px] text-muted-foreground">
                              {quoted.body}
                            </p>
                          </button>
                        ) : (
                          <div className="mb-1 border-l-2 border-primary/40 py-0.5 pl-2">
                            <p className="text-[11px] text-muted-foreground">
                              {t("quotedMissing")}
                            </p>
                          </div>
                        )
                      ) : null}
                      {grouped && event.expectsReply ? (
                        <span className="mb-0.5 inline-block rounded-full bg-amber-500/15 px-1.5 py-px text-[10px] font-medium text-amber-800 dark:text-amber-200">
                          {t("needsReplyBadge")}
                        </span>
                      ) : null}
                      <RoomPostBody
                        source={markdown}
                        onOpenSession={openSession}
                      />
                    </div>
                  </article>
                )
              })
            )}
          </div>
        </ScrollArea>
        {membersOpen ? (
          <aside className="flex w-56 shrink-0 flex-col border-l border-border/60">
            <div className="flex items-center justify-between gap-1.5 px-3 py-2 text-xs font-medium text-muted-foreground">
              <span className="flex items-center gap-1.5">
                <Users className="h-3.5 w-3.5" />
                {t("members")}
              </span>
              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                className="h-6 w-6"
                aria-label={t("addMember")}
                onClick={() => setAddOpen(true)}
              >
                <Plus className="h-3.5 w-3.5" />
              </Button>
            </div>
            <ul className="flex flex-col gap-1 px-2 pb-3">
              <li className="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs">
                <RoomSpeakerAvatar human size="sm" />
                <span className="min-w-0 flex-1 truncate font-medium">
                  {t("you")}
                </span>
              </li>
              {detail.members.map((member) => (
                <li
                  key={member.conversationId}
                  className="group flex items-center gap-1 rounded-md px-1"
                >
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-2 rounded-md px-1 py-1.5 text-left text-xs hover:bg-muted/60"
                    onClick={() => openSession(member.conversationId)}
                    title={t("openSession")}
                  >
                    <RoomSpeakerAvatar
                      human={false}
                      agentType={member.agentType}
                      size="sm"
                    />
                    <span className="min-w-0 flex-1 truncate">
                      {memberLabel(member, (id) => t("untitled", { id }))}
                    </span>
                    {runningMembers[member.conversationId] ? (
                      <span className="shrink-0 text-[10px] text-primary">
                        {t("memberRunning")}
                      </span>
                    ) : null}
                  </button>
                  <Button
                    type="button"
                    size="icon-sm"
                    variant="ghost"
                    className="h-6 w-6 opacity-0 group-hover:opacity-100"
                    aria-label={t("removeMember")}
                    onClick={() => setRemoveTarget(member)}
                  >
                    <X className="h-3 w-3" />
                  </Button>
                </li>
              ))}
            </ul>
          </aside>
        ) : null}
      </div>
      <div className="border-t border-border/60">
        <div className="mx-auto w-full max-w-3xl p-3">
          {replyTo ? (
            <div className="mb-2 flex items-center justify-between gap-2 rounded-md bg-muted/50 px-2 py-1 text-xs">
              <span className="min-w-0 truncate">
                {t("replyTo", { name: speakerName(replyTo) })}
              </span>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className="h-6 shrink-0 px-1.5 text-[11px]"
                onClick={() => setReplyTo(null)}
              >
                {t("cancelReply")}
              </Button>
            </div>
          ) : null}
          <p
            data-testid="wake-preview"
            className="mb-1.5 truncate text-[11px] text-muted-foreground"
          >
            {wakePreview.length > 0
              ? t("wakePreview", { names: wakePreview.join(", ") })
              : t("wakePreviewEmpty")}
          </p>
          <RichComposer
            ref={composerRef}
            placeholder={
              replyTo
                ? t("replyWakeHint", { name: speakerName(replyTo) })
                : t("wakeHint")
            }
            ariaLabel={t("wakeHint")}
            referenceSearch={roomMentionSearch}
            mentionUiLabels={mentionUiLabels}
            tabLabels={mentionGroupLabels}
            tabOrder={ROOM_MENTION_TAB_ORDER}
            onChange={setBody}
            className="min-h-20 rounded-md border border-input bg-transparent"
          />
          <div className="mt-1.5 flex items-center justify-end gap-2">
            <Button
              type="button"
              size="sm"
              disabled={!canPost}
              onClick={() => void handlePost()}
            >
              <Send className="h-3.5 w-3.5" />
              {t("send")}
            </Button>
          </div>
        </div>
      </div>

      <Dialog
        open={addOpen}
        onOpenChange={(open) => {
          setAddOpen(open)
          if (!open) {
            setAddSelected([])
            setAddQuery("")
          }
        }}
      >
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("addMemberTitle")}</DialogTitle>
          </DialogHeader>
          <Input
            value={addQuery}
            onChange={(event) => setAddQuery(event.target.value)}
            placeholder={t("addMemberSearch")}
          />
          <ScrollArea className="h-56">
            {candidates.length === 0 ? (
              <p className="px-1 py-6 text-center text-xs text-muted-foreground">
                {t("noCandidates")}
              </p>
            ) : (
              <ul className="flex flex-col gap-1">
                {candidates.map((conversation) => {
                  const checked = addSelected.includes(conversation.id)
                  return (
                    <li key={conversation.id}>
                      <button
                        type="button"
                        className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-muted/60"
                        onClick={() =>
                          setAddSelected((current) =>
                            current.includes(conversation.id)
                              ? current.filter((id) => id !== conversation.id)
                              : [...current, conversation.id]
                          )
                        }
                      >
                        <Checkbox checked={checked} />
                        <AgentIcon
                          agentType={conversation.agent_type}
                          className="h-3.5 w-3.5"
                        />
                        <span className="min-w-0 flex-1 truncate">
                          {conversationLabel(conversation, (id) =>
                            t("untitled", { id })
                          )}
                        </span>
                      </button>
                    </li>
                  )
                })}
              </ul>
            )}
          </ScrollArea>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddOpen(false)}>
              {t("cancel")}
            </Button>
            <Button
              disabled={addSelected.length === 0 || adding}
              onClick={() => void handleAddMembers()}
            >
              {t("add")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={pathsOpen}
        onOpenChange={(open) => {
          setPathsOpen(open)
          if (!open) setNewPath("")
        }}
      >
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("managePathsTitle")}</DialogTitle>
            <DialogDescription>{t("managePathsHint")}</DialogDescription>
          </DialogHeader>
          <div className="flex items-center gap-2">
            <Input
              value={newPath}
              onChange={(event) => setNewPath(event.target.value)}
              placeholder={t("addPathPlaceholder")}
              disabled={addingPath}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault()
                  void handleAddPath()
                }
              }}
            />
            <Button
              type="button"
              size="sm"
              disabled={!newPath.trim() || addingPath}
              onClick={() => void handleAddPath()}
            >
              {t("addPath")}
            </Button>
          </div>
          <ScrollArea className="h-56">
            {(detail.additionalPaths?.length ?? 0) === 0 ? (
              <p className="px-1 py-6 text-center text-xs text-muted-foreground">
                {t("noPaths")}
              </p>
            ) : (
              <ul className="flex flex-col gap-1">
                {detail.additionalPaths?.map((item) => (
                  <li
                    key={item.id}
                    className="group flex items-center gap-1 rounded-md px-1"
                  >
                    <span
                      className="min-w-0 flex-1 truncate px-1 py-1.5 text-left text-xs"
                      title={item.path}
                    >
                      {item.path}
                    </span>
                    <Button
                      type="button"
                      size="icon-sm"
                      variant="ghost"
                      className="h-6 w-6 opacity-0 group-hover:opacity-100"
                      aria-label={t("removePath")}
                      onClick={() => void handleRemovePath(item.id)}
                    >
                      <X className="h-3 w-3" />
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </ScrollArea>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPathsOpen(false)}>
              {t("close")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("deleteTitle")}</DialogTitle>
            <DialogDescription>{t("deleteConfirm")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setDeleteOpen(false)}
              disabled={deleting}
            >
              {t("cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={deleting}
              onClick={() => void handleDelete()}
            >
              {t("delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={removeTarget != null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setRemoveTarget(null)
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t("removeMemberTitle", {
                name: removeTarget
                  ? memberLabel(removeTarget, (id) => t("untitled", { id }))
                  : "",
              })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t("removeMemberDescription")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              disabled={removingMember}
              onClick={() => {
                if (removeTarget) {
                  void handleRemove(removeTarget.conversationId)
                }
              }}
            >
              {t("removeMember")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
