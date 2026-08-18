"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  MessagesSquare,
  Plus,
  Reply,
  Send,
  Trash2,
  UserRound,
  Users,
  X,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import { useShallow } from "zustand/react/shallow"

import { AgentIcon } from "@/components/agent-icon"
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
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Textarea } from "@/components/ui/textarea"
import { useTabActions } from "@/contexts/tab-context"
import { toErrorMessage } from "@/lib/app-error"
import { formatConversationTitle } from "@/lib/conversation-title"
import {
  addCollaborationRoomMembers,
  deleteCollaborationRoom,
  getCollaborationRoom,
  getCollaborationRoomTimeline,
  markCollaborationRoomSeen,
  postCollaborationRoomMessage,
  removeCollaborationRoomMember,
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
  insertMentionToken,
  mentionAllFromText,
  mentionMarkdownForSession,
  removeMentionToken,
  roomMessageBodyParts,
  sessionIdsFromAtAliases,
  type RoomBodyPart,
} from "@/lib/room-message-body"
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

function mentionClassName() {
  return "rounded-[3px] bg-sky-500/15 px-0.5 font-medium text-sky-800 dark:bg-sky-400/20 dark:text-sky-200"
}

function RoomMessageBody({
  parts,
  onOpenSession,
}: {
  parts: RoomBodyPart[]
  onOpenSession?: (conversationId: number) => void
}) {
  return (
    <p className="whitespace-pre-wrap text-[15px] leading-6 text-foreground">
      {parts.map((part, index) =>
        part.type === "text" ? (
          <span key={index}>{part.value}</span>
        ) : part.kind === "session" &&
          part.conversationId != null &&
          onOpenSession ? (
          <button
            key={index}
            type="button"
            className={cn(mentionClassName(), "cursor-pointer")}
            onClick={() => onOpenSession(part.conversationId!)}
          >
            {part.label}
          </button>
        ) : (
          <span key={index} className={mentionClassName()}>
            {part.label}
          </span>
        )
      )}
    </p>
  )
}

export function RoomWorkspace({ roomId }: { roomId: string }) {
  const t = useTranslations("Room")
  const { openTab, closeTab } = useTabActions()
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const collections = useCollectionStore((state) => state.items)
  const [detail, setDetail] = useState<CollaborationRoomDetail | null>(null)
  const [events, setEvents] = useState<RoomTimelineEvent[]>([])
  const [truncated, setTruncated] = useState(false)
  const [loadingOlder, setLoadingOlder] = useState(false)
  const [replyTo, setReplyTo] = useState<RoomTimelineEvent | null>(null)
  const [hydrated, setHydrated] = useState(false)
  const [body, setBody] = useState("")
  const [mentionAll, setMentionAll] = useState(false)
  const [mentioned, setMentioned] = useState<number[]>([])
  const [expectsReply, setExpectsReply] = useState(false)
  const expectsReplyTouchedRef = useRef(false)
  const [pending, setPending] = useState(false)
  const [titleDraft, setTitleDraft] = useState<string | null>(null)
  const [addOpen, setAddOpen] = useState(false)
  const [addQuery, setAddQuery] = useState("")
  const [addSelected, setAddSelected] = useState<number[]>([])
  const [adding, setAdding] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [membersOpen, setMembersOpen] = useState(false)
  const composerRef = useRef<HTMLTextAreaElement>(null)

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

  const mentionSet = useMemo(() => new Set(mentioned), [mentioned])
  const eventsById = useMemo(
    () => new Map(events.map((item) => [item.id, item])),
    [events]
  )
  const memberIds = useMemo(
    () => new Set(detail?.members.map((member) => member.conversationId) ?? []),
    [detail]
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
  const hasMention = useMemo(() => {
    if (mentionAll || mentioned.length > 0) return true
    if (mentionAllFromText(body)) return true
    if (sessionIdsFromText(body).length > 0) return true
    if (!detail) return false
    return (
      sessionIdsFromAtAliases(body, detail.members, (id) =>
        t("untitled", { id })
      ).length > 0
    )
  }, [body, detail, mentionAll, mentioned, t])
  useEffect(() => {
    if (expectsReplyTouchedRef.current) return
    setExpectsReply(hasMention)
  }, [hasMention])
  const applyComposerToken = useCallback((token: string, add: boolean) => {
    setBody((current) =>
      add
        ? insertMentionToken(current, token)
        : removeMentionToken(current, token)
    )
    queueMicrotask(() => composerRef.current?.focus())
  }, [])
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
    const mentionAllNext = mentionAll || mentionAllFromText(text)
    const targets = mentionAllNext
      ? []
      : [...new Set([...mentioned, ...mentionedFromUri, ...mentionedFromAlias])]
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
        expectsReply,
        replyToEventId: replyTo?.id ?? null,
      })
      setBody("")
      setMentioned([])
      setMentionAll(false)
      setExpectsReply(false)
      expectsReplyTouchedRef.current = false
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
  }, [
    body,
    detail,
    expectsReply,
    mentionAll,
    mentioned,
    reload,
    replyTo,
    roomId,
    t,
  ])

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
      try {
        const updated = await removeCollaborationRoomMember(
          roomId,
          conversationId
        )
        setDetail(updated)
        setMentioned((current) => current.filter((id) => id !== conversationId))
        toast.success(t("removed"))
        void useRoomCatalogStore.getState().refresh()
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
          <p className="truncate text-[11px] text-muted-foreground">
            {(detail.collectionId != null
              ? collections.find(
                  (collection) => collection.id === detail.collectionId
                )?.name
              : null) ?? t("uncategorized")}
            {" · "}
            {t("memberCount", { count: detail.members.length })}
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
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            className="h-7 w-7 text-muted-foreground hover:text-destructive"
            aria-label={t("delete")}
            onClick={() => setDeleteOpen(true)}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
      <div className="flex min-h-0 flex-1">
        <ScrollArea className="min-w-0 flex-1">
          <div className="flex flex-col py-2">
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
                const parts = roomMessageBodyParts({
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
                    className={cn(
                      "group relative flex gap-3 px-4 hover:bg-muted/40",
                      grouped ? "py-0.5" : "mt-2 py-1.5"
                    )}
                  >
                    {grouped ? (
                      <span className="w-9 shrink-0" />
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
                      <RoomMessageBody
                        parts={parts}
                        onOpenSession={openSession}
                      />
                    </div>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      className="absolute top-1.5 right-3 h-6 px-1.5 text-[11px] text-muted-foreground opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
                      title={t("replyHint")}
                      onClick={() => setReplyTo(event)}
                    >
                      <Reply className="size-3" />
                      {t("reply")}
                    </Button>
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
                    onClick={() => void handleRemove(member.conversationId)}
                  >
                    <X className="h-3 w-3" />
                  </Button>
                </li>
              ))}
            </ul>
          </aside>
        ) : null}
      </div>
      <div className="border-t border-border/60 p-3">
        <p className="mb-2 text-[11px] text-muted-foreground">
          {replyTo
            ? t("replyWakeHint", { name: speakerName(replyTo) })
            : t("wakeHint")}
        </p>
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
        <div className="mb-2 flex flex-wrap items-center gap-2 text-xs">
          <Button
            type="button"
            size="sm"
            variant={mentionAll ? "default" : "outline"}
            className="h-7 px-2 text-xs"
            onClick={() => {
              const next = !mentionAll
              setMentionAll(next)
              if (next) {
                setMentioned([])
                applyComposerToken(t("mentionAll"), true)
              } else {
                applyComposerToken(t("mentionAll"), false)
                applyComposerToken("@all", false)
                applyComposerToken("@everyone", false)
              }
            }}
          >
            {t("mentionAll")}
          </Button>
          {detail.members.map((member) => {
            const active = mentionSet.has(member.conversationId)
            return (
              <Button
                key={member.conversationId}
                type="button"
                size="sm"
                variant={active ? "default" : "outline"}
                className="h-7 px-2 text-xs"
                disabled={mentionAll}
                onClick={() => {
                  const label = memberLabel(member, (id) =>
                    t("untitled", { id })
                  )
                  const token = mentionMarkdownForSession(
                    label,
                    member.conversationId
                  )
                  const next = !active
                  setMentioned((current) =>
                    next
                      ? [...current, member.conversationId]
                      : current.filter((id) => id !== member.conversationId)
                  )
                  applyComposerToken(token, next)
                }}
              >
                @{memberLabel(member, (id) => t("untitled", { id }))}
              </Button>
            )
          })}
        </div>
        <Textarea
          ref={composerRef}
          value={body}
          onChange={(event) => setBody(event.target.value)}
          placeholder={t("composerPlaceholder")}
          className="min-h-20 resize-none"
        />
        <div className="mt-2 flex items-center justify-between gap-2">
          <label className="flex min-w-0 cursor-pointer items-start gap-2">
            <Checkbox
              checked={expectsReply}
              onCheckedChange={(checked) => {
                expectsReplyTouchedRef.current = true
                setExpectsReply(checked === true)
              }}
              aria-label={t("requestReply")}
              className="mt-0.5"
            />
            <span className="min-w-0">
              <span className="block text-xs font-medium">
                {t("requestReply")}
              </span>
              <span className="block text-[11px] text-muted-foreground">
                {t("requestReplyDescription")}
              </span>
            </span>
          </label>
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
    </div>
  )
}
