"use client"

import { useEffect, useMemo, useState } from "react"
import {
  ArrowLeft,
  ArrowUpRight,
  CornerDownRight,
  Inbox,
  Mail,
  RotateCcw,
  Search,
  Send,
  X,
} from "lucide-react"
import { useTranslations } from "next-intl"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useTabActions } from "@/contexts/tab-context"
import type { UseCollaborationFeedReturn } from "@/hooks/use-collaboration-feed"
import {
  formatConversationTitle,
  letterBodySnippet,
  letterSubjectLine,
  resolveLiveSessionTitle,
} from "@/lib/conversation-title"
import {
  isAgentUnread,
  mailHumanStatus,
  mailNoReplyNeeded,
  mailStatusVisual,
} from "@/lib/mail-human-status"
import {
  groupMailThreads,
  orderMailThreadByTree,
  summarizeMailThread,
  type MailThread,
} from "@/lib/mail-threads"
import type { CollaborationDelivery } from "@/lib/types"
import { cn } from "@/lib/utils"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import {
  SessionMailNoReplyChip,
  SessionMailStatusChip,
} from "@/components/message/session-mail-card"

const COLLABORATION_DISABLED_REASON = "session_collaboration_disabled"

/**
 * The mail-client shape: lists are folders (inbox/sent, flat, chronological),
 * and the conversation is not an entry point but the reading pane — opening
 * any letter lands in its reply-tree order, flush left. There is deliberately no third
 * "threads" tab; that role belongs to the right pane.
 */
type MailScope = "inbox" | "sent"
type MailFilter = "all" | "unread" | "needs_reply" | "awaiting"

/**
 * Display subject: only a real letter subject counts. `thread.subject` falls
 * back to a body slice for grouping, which would duplicate the snippet here.
 */
function threadSubjectOf(thread: MailThread): string | null {
  const withSubject = thread.items.find((item) => item.subject?.trim())
  return withSubject ? letterSubjectLine(withSubject.subject) || null : null
}

function newestFirst(items: CollaborationDelivery[]): CollaborationDelivery[] {
  return [...items].sort(
    (a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime()
  )
}

function formatMailTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return ""
  const now = new Date()
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  return sameDay
    ? date.toLocaleTimeString(undefined, {
        hour: "2-digit",
        minute: "2-digit",
      })
    : date.toLocaleDateString(undefined, { month: "short", day: "numeric" })
}

/**
 * Gmail-lite mailbox for one session: folder list on the left, the thread on
 * the right in tree walk order (no indent). Reading only — replies stay with
 * the agent, so the only actions are open-peer, retry, dismiss/restore.
 */
export function SessionMailboxDialog({
  open,
  onOpenChange,
  conversationId,
  collaboration,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  conversationId: number
  collaboration: UseCollaborationFeedReturn
}) {
  const t = useTranslations("Collaboration")
  const { feed, markSeen, dismiss, restore, retry } = collaboration
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const conversationById = useMemo(
    () =>
      new Map(
        conversations.map((conversation) => [conversation.id, conversation])
      ),
    [conversations]
  )
  const { openTab } = useTabActions()

  const [scope, setScope] = useState<MailScope>("inbox")
  const [filter, setFilter] = useState<MailFilter>("all")
  const [query, setQuery] = useState("")
  const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null)
  // A letter clicked in the inbox/sent scope gets highlighted in its chain.
  const [focusEventId, setFocusEventId] = useState<string | null>(null)
  // Mobile is a two-screen flow: list, then chain with a back button.
  const [mobileDetailOpen, setMobileDetailOpen] = useState(false)

  const threads = useMemo(
    () => groupMailThreads([...feed.inbound, ...feed.outbound]),
    [feed.inbound, feed.outbound]
  )
  const summaries = useMemo(
    () =>
      new Map(
        threads.map((thread) => [
          thread.rootEventId,
          summarizeMailThread(thread, conversationId),
        ])
      ),
    [threads, conversationId]
  )
  const threadRootByEventId = useMemo(() => {
    const map = new Map<string, string>()
    for (const thread of threads) {
      for (const item of thread.items) map.set(item.eventId, thread.rootEventId)
    }
    return map
  }, [threads])
  const scopeLetters = useMemo(
    () => newestFirst(scope === "inbox" ? feed.inbound : feed.outbound),
    [scope, feed.inbound, feed.outbound]
  )
  const visibleLetters = useMemo(() => {
    let letters = scopeLetters
    if (filter === "unread") letters = letters.filter(isAgentUnread)
    else if (filter === "needs_reply" || filter === "awaiting") {
      letters = letters.filter(
        (delivery) => delivery.obligationState === "awaiting_reply"
      )
    }
    const needle = query.trim().toLowerCase()
    if (!needle) return letters
    return letters.filter((delivery) => {
      const outbound = delivery.source.conversationId === conversationId
      const peerId = outbound
        ? delivery.target.conversationId
        : delivery.source.conversationId
      const peerTitle = outbound ? delivery.target.title : delivery.source.title
      return [
        delivery.subject ?? "",
        delivery.body,
        peerTitle ?? "",
        conversationById.get(peerId)?.title ?? "",
        String(peerId),
      ]
        .join("\n")
        .toLowerCase()
        .includes(needle)
    })
  }, [scopeLetters, filter, query, conversationId, conversationById])
  // Without an explicit pick, read the newest letter of the current folder.
  const fallbackRoot = scopeLetters[0]
    ? (threadRootByEventId.get(scopeLetters[0].eventId) ?? null)
    : null
  const activeThread =
    threads.find((thread) => thread.rootEventId === selectedThreadId) ??
    threads.find((thread) => thread.rootEventId === fallbackRoot) ??
    null
  const activeLetters = useMemo(
    () => (activeThread ? orderMailThreadByTree(activeThread.items) : []),
    [activeThread]
  )

  const openLetterInThread = (delivery: CollaborationDelivery) => {
    const root = threadRootByEventId.get(delivery.eventId)
    if (!root) return
    setSelectedThreadId(root)
    setFocusEventId(delivery.eventId)
    setMobileDetailOpen(true)
  }

  useEffect(() => {
    if (!focusEventId) return
    document
      .getElementById(`mailbox-letter-${focusEventId}`)
      ?.scrollIntoView?.({ block: "center" })
  }, [focusEventId])

  const unreadIds = useMemo(
    () =>
      feed.inbound
        .filter(
          (delivery) =>
            delivery.attentionState === "unread" &&
            delivery.state !== "dismissed"
        )
        .map((delivery) => delivery.id),
    [feed.inbound]
  )
  useEffect(() => {
    if (open && unreadIds.length > 0) void markSeen(unreadIds)
  }, [open, markSeen, unreadIds])

  const participantName = (
    id: number,
    snapshotTitle: string | null | undefined
  ) => {
    const live = conversationById.get(id)?.title
    if (!live?.trim() && !snapshotTitle?.trim()) return t("untitled", { id })
    return resolveLiveSessionTitle({
      conversationId: id,
      liveTitle: live,
      snapshotTitle,
    })
  }

  const openSession = (targetConversationId: number) => {
    const conversation = conversationById.get(targetConversationId)
    if (!conversation) return
    openTab(
      conversation.folder_id,
      conversation.id,
      conversation.agent_type,
      true,
      formatConversationTitle(conversation.title) || undefined
    )
  }

  const invocationState = (delivery: CollaborationDelivery) => {
    switch (delivery.interruptState) {
      case "requested":
      case "cancelling":
      case "terminal_observed":
      case "waiting_for_terminal":
        return t("stateStoppingCurrentTask")
      case "ready":
        return t("stateQueuedAfterStop")
      case "failed":
        return t("stateStopFailed")
      case "dispatching":
      case "completed":
      case null:
      case undefined:
        break
    }
    switch (delivery.state) {
      case "queued":
        if (delivery.queueState !== "paused") return t("stateQueued")
        return delivery.queuePausedReason === COLLABORATION_DISABLED_REASON
          ? t("statePausedBySettings")
          : t("stateAwaitingResumeConfirmation")
      case "embedding":
        return t("stateEmbedding")
      case "pending":
        return t("stateStoreOnly")
      case "embedded":
      case "failed":
      case "dismissed":
        return null
    }
  }

  const letterActions = (delivery: CollaborationDelivery, peerId: number) => (
    <div className="flex shrink-0 items-center">
      {conversationById.has(peerId) ? (
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="h-6 w-6"
          title={t("openSession")}
          aria-label={t("openSession")}
          onClick={() => openSession(peerId)}
        >
          <ArrowUpRight className="h-3.5 w-3.5" />
        </Button>
      ) : null}
      {(delivery.state === "failed" ||
        (delivery.state === "queued" &&
          delivery.queueState === "paused" &&
          delivery.queuePausedReason !== COLLABORATION_DISABLED_REASON)) &&
      delivery.invocationPolicy === "invoke_when_idle" ? (
        delivery.state === "queued" && delivery.queueState === "paused" ? (
          <Button
            type="button"
            size="sm"
            variant="ghost"
            className="h-6 px-2 text-[11px]"
            title={t("startProcessing")}
            aria-label={t("startProcessing")}
            onClick={() => void retry(delivery.queueItemId || delivery.id)}
          >
            {t("startProcessing")}
          </Button>
        ) : (
          <Button
            type="button"
            size="icon"
            variant="ghost"
            className="h-6 w-6"
            title={t("retry")}
            aria-label={t("retry")}
            onClick={() => void retry(delivery.queueItemId || delivery.id)}
          >
            <RotateCcw className="h-3.5 w-3.5" />
          </Button>
        )
      ) : null}
      {delivery.state === "pending" ? (
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="h-6 w-6"
          title={t("dismiss")}
          aria-label={t("dismiss")}
          onClick={() => void dismiss(delivery.id)}
        >
          <X className="h-3.5 w-3.5" />
        </Button>
      ) : null}
      {delivery.state === "dismissed" &&
      delivery.invocationPolicy === "store_only" ? (
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="h-6 w-6"
          title={t("restore")}
          aria-label={t("restore")}
          onClick={() => void restore(delivery.id)}
        >
          <RotateCcw className="h-3.5 w-3.5" />
        </Button>
      ) : null}
    </div>
  )

  /** Flat inbox/sent row: one letter, newest first; click opens its chain. */
  const letterRow = (delivery: CollaborationDelivery) => {
    const outbound = delivery.source.conversationId === conversationId
    const direction = outbound ? "outbound" : "inbound"
    const peerId = outbound
      ? delivery.target.conversationId
      : delivery.source.conversationId
    const peerTitle = outbound ? delivery.target.title : delivery.source.title
    const unread = !outbound && isAgentUnread(delivery)
    const status = mailHumanStatus(delivery)
    const tone = mailStatusVisual(status)
    const subject = letterSubjectLine(delivery.subject)
    const snippet = letterBodySnippet(delivery.body, 56)
    return (
      <button
        key={delivery.id}
        type="button"
        data-letter-row={direction}
        data-mail-status={status}
        className={cn(
          "flex w-full items-stretch border-b border-border/50 text-left last:border-b-0",
          focusEventId === delivery.eventId &&
            activeThread?.rootEventId ===
              threadRootByEventId.get(delivery.eventId)
            ? "bg-muted/60"
            : "hover:bg-muted/35"
        )}
        onClick={() => openLetterInThread(delivery)}
      >
        <span className={`w-0.5 shrink-0 ${tone.bar}`} />
        <span className="min-w-0 flex-1 px-2.5 py-2">
          <span className="flex items-baseline gap-2">
            <span
              className={cn(
                "min-w-0 flex-1 truncate text-[11px]",
                unread
                  ? "font-semibold text-foreground"
                  : "text-muted-foreground"
              )}
            >
              {participantName(peerId, peerTitle)}
            </span>
            <span className="shrink-0 text-[10px] text-muted-foreground">
              {formatMailTime(delivery.createdAt)}
            </span>
          </span>
          <span className="mt-0.5 flex items-center gap-2">
            <span className="min-w-0 flex-1 truncate text-[12px] leading-5">
              <span className={unread ? "font-semibold" : "font-medium"}>
                {subject || t("untitledSubject")}
              </span>
              {snippet ? (
                <span className="text-muted-foreground"> — {snippet}</span>
              ) : null}
            </span>
            <SessionMailStatusChip status={status} direction={direction} />
          </span>
        </span>
      </button>
    )
  }

  const letterCard = (thread: MailThread, delivery: CollaborationDelivery) => {
    const outbound = delivery.source.conversationId === conversationId
    const direction = outbound ? "outbound" : "inbound"
    const peerId = outbound
      ? delivery.target.conversationId
      : delivery.source.conversationId
    const status = mailHumanStatus(delivery)
    const tone = mailStatusVisual(status)
    const state = invocationState(delivery)
    const subject = letterSubjectLine(delivery.subject)
    const parent = delivery.replyToEventId
      ? (thread.items.find(
          (item) => item.eventId === delivery.replyToEventId
        ) ?? null)
      : null
    const DirectionIcon = outbound ? Send : Inbox
    return (
      <article
        key={delivery.id}
        id={`mailbox-letter-${delivery.eventId}`}
        data-thread-letter={direction}
        data-mail-status={status}
        className={cn(
          "overflow-hidden rounded-lg border",
          outbound
            ? "border-border bg-muted/45"
            : "border-sky-500/30 bg-sky-500/[0.05]",
          focusEventId === delivery.eventId && "ring-2 ring-primary/35"
        )}
      >
        <div className="flex">
          <span className={`w-0.5 shrink-0 ${tone.bar}`} />
          <div className="min-w-0 flex-1 px-3 py-2">
            <header className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
              <DirectionIcon className="h-3 w-3 shrink-0" aria-hidden="true" />
              <span className="font-medium text-foreground">
                {outbound
                  ? t("to", {
                      name: participantName(peerId, delivery.target.title),
                    })
                  : t("from", {
                      name: participantName(peerId, delivery.source.title),
                    })}
              </span>
              <SessionMailStatusChip status={status} direction={direction} />
              {mailNoReplyNeeded(delivery) ? <SessionMailNoReplyChip /> : null}
              {delivery.obligationState === "awaiting_reply" &&
              status === "unread" ? (
                <span className="rounded bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-800 dark:text-amber-300">
                  {outbound ? t("stateAwaitingReply") : t("stateNeedsReply")}
                </span>
              ) : null}
              {state ? <span>{state}</span> : null}
              <span className="ml-auto flex shrink-0 items-center gap-1">
                <span className="text-[10px]">
                  {formatMailTime(delivery.createdAt)}
                </span>
                {letterActions(delivery, peerId)}
              </span>
            </header>
            {parent ? (
              <button
                type="button"
                className="mt-1 inline-flex max-w-full items-center gap-1 truncate text-left text-[11px] text-muted-foreground hover:text-foreground"
                onClick={() =>
                  document
                    .getElementById(`mailbox-letter-${parent.eventId}`)
                    ?.scrollIntoView?.({ block: "center" })
                }
              >
                <CornerDownRight className="h-3 w-3 shrink-0" />
                <span className="truncate">
                  {t("replyToSubject", {
                    subject:
                      letterSubjectLine(parent.subject) ||
                      letterBodySnippet(parent.body, 32) ||
                      parent.eventId.slice(0, 8),
                  })}
                </span>
              </button>
            ) : null}
            {subject ? (
              <p
                data-collaboration-subject=""
                className="mt-1 text-[13px] font-semibold leading-snug"
              >
                {subject}
              </p>
            ) : null}
            <p className="mt-1 text-[13px] leading-relaxed whitespace-pre-wrap break-words">
              {delivery.body}
            </p>
            {delivery.error ? (
              <p className="mt-1 break-words text-[11px] text-destructive">
                {delivery.error}
              </p>
            ) : null}
            {delivery.interruptError ? (
              <p className="mt-1 break-words text-[11px] text-destructive">
                {delivery.interruptError}
              </p>
            ) : null}
          </div>
        </div>
      </article>
    )
  }

  const activeSummary = activeThread
    ? summaries.get(activeThread.rootEventId)
    : null

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        data-mailbox-panel=""
        className="flex h-[min(42rem,88dvh)] w-[min(60rem,95vw)] max-w-none flex-col gap-0 overflow-hidden rounded-2xl p-0"
        // Keep the conversation page's right-click menu out of the mailbox;
        // portals still bubble React events to the tab ancestors.
        onContextMenu={(event) => event.stopPropagation()}
      >
        <DialogHeader className="shrink-0 border-b px-4 py-2.5">
          <DialogTitle className="flex items-center gap-2 text-sm">
            <Mail
              className="h-4 w-4 text-muted-foreground"
              aria-hidden="true"
            />
            {t("panelTitle")}
            <span className="text-[11px] font-normal text-muted-foreground">
              {t("inboxCount", { count: feed.inbound.length })}
              {feed.outbound.length > 0
                ? ` · ${t("sentCount", { count: feed.outbound.length })}`
                : ""}
            </span>
          </DialogTitle>
        </DialogHeader>
        <div className="flex min-h-0 flex-1">
          <div
            className={cn(
              "w-full flex-col border-border/60 sm:flex sm:w-72 sm:shrink-0 sm:border-r",
              mobileDetailOpen ? "hidden" : "flex"
            )}
          >
            <div
              role="tablist"
              aria-label={t("panelTitle")}
              className="flex shrink-0 gap-0.5 border-b bg-muted/40 p-1"
            >
              {(
                [
                  ["inbox", t("inboxLabel"), Inbox],
                  ["sent", t("sentLabel"), Send],
                ] as const
              ).map(([id, label, Icon]) => (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  aria-selected={scope === id}
                  className={cn(
                    "inline-flex flex-1 items-center justify-center gap-1 rounded-md px-2 py-1 text-[11px] font-medium",
                    scope === id
                      ? "bg-background text-foreground shadow-sm"
                      : "text-muted-foreground hover:text-foreground"
                  )}
                  onClick={() => {
                    setScope(id)
                    setFilter("all")
                  }}
                >
                  <Icon className="h-3 w-3" aria-hidden="true" />
                  {label}
                </button>
              ))}
            </div>
            <div className="shrink-0 border-b border-border/60 px-2 py-1.5">
              <div className="flex items-center gap-1.5 rounded-md bg-muted/50 px-2">
                <Search
                  className="h-3 w-3 shrink-0 text-muted-foreground"
                  aria-hidden="true"
                />
                <input
                  type="text"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder={t("mailSearchPlaceholder")}
                  aria-label={t("mailSearchPlaceholder")}
                  className="h-6 w-full bg-transparent text-[12px] outline-none placeholder:text-muted-foreground"
                />
              </div>
            </div>
            <div
              role="radiogroup"
              aria-label={scope === "inbox" ? t("inboxLabel") : t("sentLabel")}
              className="flex shrink-0 gap-0.5 border-b border-border/60 px-1 py-1"
            >
              {(scope === "inbox"
                ? ([
                    ["all", t("mailFilterAll")],
                    ["unread", t("mailUnread")],
                    ["needs_reply", t("stateNeedsReply")],
                  ] as const)
                : ([
                    ["all", t("mailFilterAll")],
                    ["awaiting", t("stateAwaitingReply")],
                  ] as const)
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  role="radio"
                  aria-checked={filter === id}
                  className={cn(
                    "rounded-full px-2 py-0.5 text-[10px] font-medium",
                    filter === id
                      ? "bg-muted text-foreground"
                      : "text-muted-foreground hover:text-foreground"
                  )}
                  onClick={() => setFilter(id)}
                >
                  {label}
                </button>
              ))}
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto">
              {visibleLetters.length === 0 ? (
                <p className="px-3 py-8 text-center text-xs text-muted-foreground">
                  {scopeLetters.length > 0
                    ? t("mailFilterEmpty")
                    : scope === "inbox"
                      ? t("inboxEmpty")
                      : t("sentEmpty")}
                </p>
              ) : (
                visibleLetters.map(letterRow)
              )}
            </div>
          </div>
          <div
            className={cn(
              "min-w-0 flex-1 flex-col sm:flex",
              mobileDetailOpen ? "flex" : "hidden"
            )}
          >
            {activeThread ? (
              <>
                <header className="shrink-0 border-b border-border/60 px-4 py-2.5">
                  <div className="flex items-start gap-2">
                    <Button
                      type="button"
                      size="icon"
                      variant="ghost"
                      className="h-6 w-6 shrink-0 sm:hidden"
                      title={t("mailBackToList")}
                      aria-label={t("mailBackToList")}
                      onClick={() => setMobileDetailOpen(false)}
                    >
                      <ArrowLeft className="h-3.5 w-3.5" />
                    </Button>
                    <div className="min-w-0 flex-1">
                      <p className="line-clamp-2 text-sm font-semibold leading-snug">
                        {threadSubjectOf(activeThread) || t("untitledSubject")}
                      </p>
                      <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
                        <span>
                          {t("threadCount", {
                            count: activeThread.items.length,
                          })}
                        </span>
                        {activeSummary?.peerIds.map((peerId) => (
                          <button
                            key={peerId}
                            type="button"
                            className="inline-flex items-center gap-0.5 rounded border border-border/70 px-1.5 py-0.5 hover:bg-muted/60 hover:text-foreground disabled:pointer-events-none"
                            disabled={!conversationById.has(peerId)}
                            onClick={() => openSession(peerId)}
                          >
                            {participantName(peerId, null)}
                            {conversationById.has(peerId) ? (
                              <ArrowUpRight
                                className="h-3 w-3"
                                aria-hidden="true"
                              />
                            ) : null}
                          </button>
                        ))}
                      </div>
                    </div>
                  </div>
                </header>
                <div className="min-h-0 flex-1 space-y-2.5 overflow-y-auto px-4 py-3">
                  {activeLetters.map((delivery) =>
                    letterCard(activeThread, delivery)
                  )}
                </div>
              </>
            ) : (
              <p className="flex flex-1 items-center justify-center px-6 text-center text-xs text-muted-foreground">
                {threads.length === 0
                  ? t("threadsEmpty")
                  : t("mailFilterEmpty")}
              </p>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
