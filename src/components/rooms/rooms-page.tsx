"use client"

import { useCallback, useEffect, useMemo, useState } from "react"
import { MessagesSquare, Send, Users } from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { WorkbenchPageTitle } from "@/components/workbench/workbench-page-title"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Textarea } from "@/components/ui/textarea"
import { useTabActions } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import { toErrorMessage } from "@/lib/app-error"
import { formatConversationTitle } from "@/lib/conversation-title"
import {
  getCollaborationRoom,
  getCollaborationRoomTimeline,
  listCollaborationRooms,
  markCollaborationRoomSeen,
  postCollaborationRoomMessage,
} from "@/lib/api"
import { subscribe } from "@/lib/platform"
import { OPEN_ROOM_EVENT, ROOM_CHANGED_EVENT } from "@/lib/room-events"
import { cn } from "@/lib/utils"
import type {
  AgentType,
  CollaborationRoomDetail,
  CollaborationRoomMember,
  CollaborationRoomSummary,
  RoomChanged,
  RoomTimelineEvent,
} from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useTabStore } from "@/stores/tab-store"

function memberLabel(
  member: Pick<CollaborationRoomMember, "conversationId" | "title">,
  untitled: (id: number) => string
) {
  const title = member.title?.trim()
  return title
    ? formatConversationTitle(title)
    : untitled(member.conversationId)
}

export function RoomsPageTitle() {
  const t = useTranslations("Room")
  return <WorkbenchPageTitle title={t("title")} />
}

export function RoomsPage() {
  const t = useTranslations("Room")
  const { openTab } = useTabActions()
  const { openConversations } = useWorkbenchRoute()
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const workbenchId = useTabStore((state) => state.activeWorkbenchId)
  const [rooms, setRooms] = useState<CollaborationRoomSummary[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<CollaborationRoomDetail | null>(null)
  const [events, setEvents] = useState<RoomTimelineEvent[]>([])
  const [hydrated, setHydrated] = useState(false)
  const [body, setBody] = useState("")
  const [mentionAll, setMentionAll] = useState(false)
  const [mentioned, setMentioned] = useState<number[]>([])
  const [invokeWhenIdle, setInvokeWhenIdle] = useState(true)
  const [expectsReply, setExpectsReply] = useState(false)
  const [speakingAs, setSpeakingAs] = useState<number | null>(null)
  const [pending, setPending] = useState(false)

  const reloadList = useCallback(async () => {
    const next = await listCollaborationRooms(workbenchId)
    setRooms(next)
    setHydrated(true)
    return next
  }, [workbenchId])

  const reloadSelected = useCallback(async (roomId: string) => {
    const [nextDetail, timeline] = await Promise.all([
      getCollaborationRoom(roomId),
      getCollaborationRoomTimeline(roomId),
    ])
    setDetail(nextDetail)
    setEvents(timeline.events)
    setSpeakingAs((current) => {
      if (
        current != null &&
        nextDetail.members.some((member) => member.conversationId === current)
      ) {
        return current
      }
      return nextDetail.createdByConversationId
    })
    void markCollaborationRoomSeen(roomId).catch(() => undefined)
  }, [])

  useEffect(() => {
    let cancelled = false
    void reloadList()
      .then((next) => {
        if (cancelled) return
        setSelectedId((current) => current ?? next[0]?.id ?? null)
      })
      .catch((error) => {
        if (!cancelled) {
          toast.error(toErrorMessage(error))
          setHydrated(true)
        }
      })
    return () => {
      cancelled = true
    }
  }, [reloadList])

  useEffect(() => {
    if (!selectedId) {
      setDetail(null)
      setEvents([])
      return
    }
    void reloadSelected(selectedId).catch((error) => {
      toast.error(toErrorMessage(error))
    })
  }, [reloadSelected, selectedId])

  useEffect(() => {
    let disposed = false
    let unsubscribe: (() => void) | undefined
    void subscribe<RoomChanged>(ROOM_CHANGED_EVENT, (change) => {
      if (disposed || change.workbenchId !== workbenchId) return
      void reloadList().catch(() => undefined)
      if (change.roomId === selectedId) {
        void reloadSelected(change.roomId).catch(() => undefined)
      }
    }).then((off) => {
      if (disposed) off()
      else unsubscribe = off
    })
    const onOpen = (event: Event) => {
      const roomId = (event as CustomEvent<{ roomId: string }>).detail?.roomId
      if (roomId) setSelectedId(roomId)
    }
    window.addEventListener(OPEN_ROOM_EVENT, onOpen)
    return () => {
      disposed = true
      unsubscribe?.()
      window.removeEventListener(OPEN_ROOM_EVENT, onOpen)
    }
  }, [reloadList, reloadSelected, selectedId, workbenchId])

  const mentionSet = useMemo(() => new Set(mentioned), [mentioned])
  const canPost =
    Boolean(selectedId && speakingAs && body.trim()) && !pending && hydrated

  const handlePost = useCallback(async () => {
    if (!selectedId || speakingAs == null || !body.trim()) return
    setPending(true)
    try {
      await postCollaborationRoomMessage({
        roomId: selectedId,
        sourceConversationId: speakingAs,
        targetConversationIds: mentionAll ? [] : mentioned,
        mentionAll,
        subject: body.trim().slice(0, 80),
        body: body.trim(),
        clientDedupeId: crypto.randomUUID(),
        invocationPolicy:
          mentionAll || mentioned.length > 0
            ? invokeWhenIdle
              ? "invoke_when_idle"
              : "store_only"
            : "store_only",
        expectsReply: (mentionAll || mentioned.length > 0) && expectsReply,
      })
      setBody("")
      setMentioned([])
      setMentionAll(false)
      toast.success(t("posted"))
      await reloadSelected(selectedId)
      await reloadList()
    } catch (error) {
      toast.error(t("sendFailed"))
      console.error(toErrorMessage(error))
    } finally {
      setPending(false)
    }
  }, [
    body,
    expectsReply,
    invokeWhenIdle,
    mentionAll,
    mentioned,
    reloadList,
    reloadSelected,
    selectedId,
    speakingAs,
    t,
  ])

  return (
    <div className="flex h-full min-h-0">
      <aside className="flex w-64 shrink-0 flex-col border-r border-border/60">
        <div className="flex h-10 items-center gap-2 px-3 text-xs font-medium text-muted-foreground">
          <Users className="h-3.5 w-3.5" />
          {t("title")}
        </div>
        <ScrollArea className="min-h-0 flex-1">
          {hydrated && rooms.length === 0 ? (
            <p className="px-3 py-6 text-sm text-muted-foreground">
              {t("empty")}
            </p>
          ) : (
            <ul className="flex flex-col gap-0.5 p-1.5">
              {rooms.map((room) => (
                <li key={room.id}>
                  <button
                    type="button"
                    onClick={() => setSelectedId(room.id)}
                    className={cn(
                      "flex w-full flex-col rounded-md px-2.5 py-2 text-left",
                      selectedId === room.id
                        ? "bg-sidebar-primary/10"
                        : "hover:bg-muted/60"
                    )}
                  >
                    <span className="truncate text-sm font-medium">
                      {room.title}
                    </span>
                    <span className="text-[11px] text-muted-foreground">
                      {t("memberCount", { count: room.memberCount })}
                      {room.unreadCount > 0
                        ? ` · ${t("unread", { count: room.unreadCount })}`
                        : ""}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </ScrollArea>
      </aside>
      <section className="flex min-w-0 flex-1 flex-col">
        {!detail ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 px-8 text-center text-sm text-muted-foreground">
            <MessagesSquare className="h-6 w-6 opacity-50" />
            <p>{t("emptyHint")}</p>
          </div>
        ) : (
          <>
            <div className="flex min-h-10 items-center justify-between gap-3 border-b border-border/60 px-4">
              <div className="min-w-0">
                <h2 className="truncate text-sm font-semibold">
                  {detail.title}
                </h2>
                <p className="truncate text-[11px] text-muted-foreground">
                  {detail.members
                    .map((member) =>
                      memberLabel(member, (id) => t("untitled", { id }))
                    )
                    .join(" · ")}
                </p>
              </div>
            </div>
            <div className="flex min-h-0 flex-1">
              <ScrollArea className="min-w-0 flex-1">
                <div className="flex flex-col gap-3 p-4">
                  {events.length === 0 ? (
                    <p className="text-sm text-muted-foreground">
                      {t("timelineEmpty")}
                    </p>
                  ) : (
                    events.map((event) => {
                      const names = event.mentionConversationIds
                        .map((id) => {
                          const member = detail.members.find(
                            (item) => item.conversationId === id
                          )
                          return member
                            ? memberLabel(member, (mid) =>
                                t("untitled", { id: mid })
                              )
                            : t("untitled", { id })
                        })
                        .join(", ")
                      return (
                        <article
                          key={event.id}
                          className="rounded-lg border border-border/60 bg-background px-3 py-2"
                        >
                          <div className="mb-1 flex items-center justify-between gap-2 text-[11px] text-muted-foreground">
                            <span>
                              {memberLabel(
                                {
                                  conversationId: event.source.conversationId,
                                  title: event.source.title,
                                },
                                (id) => t("untitled", { id })
                              )}
                            </span>
                            {names ? (
                              <span>{t("mentioned", { names })}</span>
                            ) : null}
                          </div>
                          {event.subject ? (
                            <p className="text-xs font-medium">
                              {event.subject}
                            </p>
                          ) : null}
                          <p className="whitespace-pre-wrap text-sm">
                            {event.body}
                          </p>
                        </article>
                      )
                    })
                  )}
                </div>
              </ScrollArea>
              <aside className="flex w-52 shrink-0 flex-col border-l border-border/60">
                <div className="px-3 py-2 text-xs font-medium text-muted-foreground">
                  {t("members")}
                </div>
                <ul className="flex flex-col gap-1 px-2 pb-3">
                  {detail.members.map((member) => (
                    <li key={member.conversationId}>
                      <button
                        type="button"
                        className="flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-xs hover:bg-muted/60"
                        onClick={() => {
                          const live = conversations.find(
                            (conversation) =>
                              conversation.id === member.conversationId
                          )
                          if (!live) return
                          openConversations()
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
                        <span className="truncate">
                          {memberLabel(member, (id) => t("untitled", { id }))}
                        </span>
                        <span className="text-[10px] text-muted-foreground">
                          {member.role}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              </aside>
            </div>
            <div className="border-t border-border/60 p-3">
              <div className="mb-2 flex flex-wrap items-center gap-2 text-xs">
                <label className="flex items-center gap-1.5">
                  <span className="text-muted-foreground">
                    {t("speakingAs")}
                  </span>
                  <select
                    className="h-7 rounded-md border border-input bg-background px-1.5"
                    value={speakingAs ?? ""}
                    onChange={(event) =>
                      setSpeakingAs(Number(event.target.value))
                    }
                  >
                    {detail.members.map((member) => (
                      <option
                        key={member.conversationId}
                        value={member.conversationId}
                      >
                        {memberLabel(member, (id) => t("untitled", { id }))}
                      </option>
                    ))}
                  </select>
                </label>
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
                {detail.members
                  .filter((member) => member.conversationId !== speakingAs)
                  .map((member) => {
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
                              ? current.filter(
                                  (id) => id !== member.conversationId
                                )
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
              <div className="mt-2 flex items-center justify-between gap-2">
                <div className="flex items-center gap-3 text-xs text-muted-foreground">
                  <label className="flex items-center gap-1.5">
                    <Checkbox
                      checked={invokeWhenIdle}
                      onCheckedChange={(value) =>
                        setInvokeWhenIdle(value === true)
                      }
                      disabled={!mentionAll && mentioned.length === 0}
                    />
                    {t("invokeWhenIdle")}
                  </label>
                  <label className="flex items-center gap-1.5">
                    <Checkbox
                      checked={expectsReply}
                      onCheckedChange={(value) =>
                        setExpectsReply(value === true)
                      }
                      disabled={!mentionAll && mentioned.length === 0}
                    />
                    {t("requestReply")}
                  </label>
                </div>
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
          </>
        )}
      </section>
    </div>
  )
}
