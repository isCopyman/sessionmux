"use client"

import { useCallback, useEffect, useMemo, useState } from "react"
import { MessagesSquare, Plus, Send, UserRound, Users, X } from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { AgentIcon } from "@/components/agent-icon"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
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
import { useRoomCatalogStore } from "@/stores/room-catalog-store"
import type {
  AgentType,
  CollaborationRoomDetail,
  CollaborationRoomMember,
  DbConversationSummary,
  RoomChanged,
  RoomTimelineEvent,
} from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"

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

export function RoomWorkspace({ roomId }: { roomId: string }) {
  const t = useTranslations("Room")
  const { openTab } = useTabActions()
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const [detail, setDetail] = useState<CollaborationRoomDetail | null>(null)
  const [events, setEvents] = useState<RoomTimelineEvent[]>([])
  const [truncated, setTruncated] = useState(false)
  const [loadingOlder, setLoadingOlder] = useState(false)
  const [replyTo, setReplyTo] = useState<RoomTimelineEvent | null>(null)
  const [hydrated, setHydrated] = useState(false)
  const [body, setBody] = useState("")
  const [mentionAll, setMentionAll] = useState(false)
  const [mentionHuman, setMentionHuman] = useState(false)
  const [mentioned, setMentioned] = useState<number[]>([])
  const [pending, setPending] = useState(false)
  const [titleDraft, setTitleDraft] = useState<string | null>(null)
  const [addOpen, setAddOpen] = useState(false)
  const [addQuery, setAddQuery] = useState("")
  const [addSelected, setAddSelected] = useState<number[]>([])
  const [adding, setAdding] = useState(false)

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
  const memberIds = useMemo(
    () => new Set(detail?.members.map((member) => member.conversationId) ?? []),
    [detail]
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
    const mentionedFromUri = sessionIdsFromText(text)
    const targets = mentionAll
      ? []
      : [...new Set([...mentioned, ...mentionedFromUri])]
    const mentionHumanNext = mentionHuman || mentionsHumanFromText(text)
    setPending(true)
    try {
      await postCollaborationRoomMessage({
        roomId,
        sourceConversationId: detail.createdByConversationId,
        targetConversationIds: targets,
        mentionAll,
        mentionHuman: mentionHumanNext,
        authorKind: "human",
        subject: text.slice(0, 80),
        body: text,
        clientDedupeId: crypto.randomUUID(),
        invocationPolicy:
          mentionAll || targets.length > 0 ? "invoke_when_idle" : "store_only",
        expectsReply: mentionAll || targets.length > 0,
        replyToEventId: replyTo?.id ?? null,
      })
      setBody("")
      setMentioned([])
      setMentionAll(false)
      setMentionHuman(false)
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
    mentionAll,
    mentionHuman,
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
              <span className="inline-flex items-center rounded-full bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                {t("roomBadge")}
              </span>
              <h2 className="truncate text-sm font-semibold">{detail.title}</h2>
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
            {t("youHost")} ·{" "}
            {t("memberCount", { count: detail.members.length })}
          </p>
        </div>
      </div>
      <div className="flex min-h-0 flex-1">
        <ScrollArea className="min-w-0 flex-1">
          <div className="flex flex-col gap-3 p-4">
            {truncated ? (
              <div className="flex justify-center">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className="h-7 px-2 text-xs"
                  disabled={loadingOlder}
                  onClick={() => void loadOlder()}
                >
                  {t("loadOlder")}
                </Button>
              </div>
            ) : null}
            {events.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                {t("timelineEmpty")}
              </p>
            ) : (
              events.map((event) => {
                const fromYou = event.authorKind === "human"
                const names = event.mentionConversationIds
                  .map((id) => {
                    const member = detail.members.find(
                      (item) => item.conversationId === id
                    )
                    return member
                      ? memberLabel(member, (mid) => t("untitled", { id: mid }))
                      : t("untitled", { id })
                  })
                  .join(", ")
                return (
                  <article
                    key={event.id}
                    className={cn(
                      "max-w-[85%] rounded-2xl px-3 py-2",
                      fromYou
                        ? "self-end bg-primary/10"
                        : "self-start border border-border/60 bg-background"
                    )}
                  >
                    <div className="mb-1 flex items-center gap-1.5 text-[11px] text-muted-foreground">
                      {fromYou ? (
                        <UserRound className="h-3 w-3" />
                      ) : (
                        <AgentIcon
                          agentType={
                            (event.source.agentType ??
                              "claude_code") as AgentType
                          }
                          className="h-3 w-3"
                        />
                      )}
                      <span>
                        {fromYou
                          ? t("you")
                          : memberLabel(
                              {
                                conversationId: event.source.conversationId,
                                title: event.source.title,
                              },
                              (id) => t("untitled", { id })
                            )}
                      </span>
                      {event.mentionHuman ? (
                        <span>{t("mentionHuman")}</span>
                      ) : null}
                      {names ? (
                        <span className="ms-auto">
                          {t("mentioned", { names })}
                        </span>
                      ) : null}
                    </div>
                    <p className="whitespace-pre-wrap text-sm">{event.body}</p>
                    <div className="mt-1 flex justify-end">
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        className="h-6 px-1.5 text-[11px] text-muted-foreground"
                        onClick={() => setReplyTo(event)}
                      >
                        {t("reply")}
                      </Button>
                    </div>
                  </article>
                )
              })
            )}
          </div>
        </ScrollArea>
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
              <UserRound className="h-3.5 w-3.5 text-primary" />
              <span className="min-w-0 flex-1 truncate font-medium">
                {t("you")}
              </span>
              <span className="text-[10px] text-muted-foreground">
                {t("youHint")}
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
                  onClick={() => {
                    const live = conversations.find(
                      (conversation) =>
                        conversation.id === member.conversationId
                    )
                    if (!live) return
                    openTab(
                      live.folder_id,
                      live.id,
                      live.agent_type as AgentType,
                      true,
                      formatConversationTitle(live.title) || undefined
                    )
                  }}
                  title={t("openSession")}
                >
                  <AgentIcon
                    agentType={(member.agentType ?? "claude_code") as AgentType}
                    className="h-3.5 w-3.5"
                  />
                  <span className="min-w-0 flex-1 truncate">
                    {memberLabel(member, (id) => t("untitled", { id }))}
                  </span>
                  <span className="text-[10px] text-muted-foreground">
                    {member.role === "owner" ? t("roleOwner") : t("roleMember")}
                  </span>
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
      </div>
      <div className="border-t border-border/60 p-3">
        <p className="mb-2 text-[11px] text-muted-foreground">
          {t("wakeHint")}
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
              setMentionAll((value) => !value)
              if (!mentionAll) setMentioned([])
            }}
          >
            {t("mentionAll")}
          </Button>
          <Button
            type="button"
            size="sm"
            variant={mentionHuman ? "default" : "outline"}
            className="h-7 px-2 text-xs"
            onClick={() => setMentionHuman((value) => !value)}
          >
            {t("mentionHuman")}
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
                onClick={() =>
                  setMentioned((current) =>
                    current.includes(member.conversationId)
                      ? current.filter((id) => id !== member.conversationId)
                      : [...current, member.conversationId]
                  )
                }
              >
                @{memberLabel(member, (id) => t("untitled", { id }))}
              </Button>
            )
          })}
        </div>
        <Textarea
          value={body}
          onChange={(event) => setBody(event.target.value)}
          placeholder={t("composerPlaceholder")}
          className="min-h-20"
        />
        <div className="mt-2 flex items-center justify-end gap-2">
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
    </div>
  )
}
