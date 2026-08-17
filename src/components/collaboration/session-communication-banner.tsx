"use client"

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import { ArrowUpRight, Inbox, Mail, RotateCcw, Send, X } from "lucide-react"
import { useTranslations } from "next-intl"
import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { useTabActions } from "@/contexts/tab-context"
import {
  useCollaborationFeed,
  type UseCollaborationFeedReturn,
} from "@/hooks/use-collaboration-feed"
import {
  formatConversationTitle,
  letterBodySnippet,
  letterListPreview,
  letterSubjectLine,
  resolveLiveSessionTitle,
} from "@/lib/conversation-title"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import type { CollaborationDelivery } from "@/lib/types"
import {
  isAgentUnread,
  mailHumanStatus,
  mailStatusVisual,
  type MailHumanStatus,
} from "@/lib/mail-human-status"
import { groupMailThreads, type MailThread } from "@/lib/mail-threads"
const COLLABORATION_DISABLED_REASON = "session_collaboration_disabled"

interface SessionCommunicationBannerProps {
  conversationId: number | null
}

interface SessionCommunicationBannerViewProps extends SessionCommunicationBannerProps {
  collaboration: UseCollaborationFeedReturn
}

function participantLabel(
  conversationId: number,
  snapshotTitle: string | null | undefined,
  liveTitle: string | null | undefined,
  untitled: (values: { id: number }) => string
) {
  const live = liveTitle?.trim()
  const snapshot = snapshotTitle?.trim()
  if (!live && !snapshot) return untitled({ id: conversationId })
  return resolveLiveSessionTitle({
    conversationId,
    liveTitle,
    snapshotTitle,
  })
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

function subjectOf(delivery: CollaborationDelivery, untitled: string): string {
  return letterSubjectLine(delivery.subject) || untitled
}

export function SessionCommunicationBanner({
  conversationId,
}: SessionCommunicationBannerProps) {
  const collaboration = useCollaborationFeed(conversationId)
  return (
    <SessionCommunicationBannerView
      conversationId={conversationId}
      collaboration={collaboration}
    />
  )
}

export function SessionCommunicationBannerView({
  conversationId,
  collaboration,
}: SessionCommunicationBannerViewProps) {
  const t = useTranslations("Collaboration")
  const [expanded, setExpanded] = useState(false)
  const [view, setView] = useState<"inbox" | "sent" | "threads">("inbox")
  const [selectedDeliveryId, setSelectedDeliveryId] = useState<string | null>(
    null
  )
  const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const conversationById = useMemo(
    () =>
      new Map(
        conversations.map((conversation) => [conversation.id, conversation])
      ),
    [conversations]
  )
  const { openTab } = useTabActions()
  const { feed, hydrated, markSeen, dismiss, restore, retry } = collaboration
  const total = feed.inbound.length + feed.outbound.length
  const inboxUnread = feed.inbound.filter(isAgentUnread).length
  const inboxAwaiting = feed.inbound.filter(
    (delivery) => delivery.obligationState === "awaiting_reply"
  ).length
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
  const threads = useMemo(
    () => groupMailThreads([...feed.inbound, ...feed.outbound]),
    [feed.inbound, feed.outbound]
  )

  useEffect(() => {
    if (expanded && unreadIds.length > 0) void markSeen(unreadIds)
  }, [expanded, markSeen, unreadIds])

  useEffect(() => {
    const node = triggerRef.current
    if (!node || !expanded) return
    if (typeof IntersectionObserver === "undefined") return
    let seenVisible = false
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry) return
        if (entry.isIntersecting) {
          seenVisible = true
          return
        }
        if (seenVisible) setExpanded(false)
      },
      { threshold: 0.05 }
    )
    observer.observe(node)
    return () => observer.disconnect()
  }, [expanded])

  const visibleDeliveryIds =
    view === "inbox"
      ? feed.inbound.map((delivery) => delivery.id)
      : view === "sent"
        ? feed.outbound.map((delivery) => delivery.id)
        : []
  const activeDeliveryId =
    selectedDeliveryId && visibleDeliveryIds.includes(selectedDeliveryId)
      ? selectedDeliveryId
      : (visibleDeliveryIds[0] ?? null)
  const activeThreadId =
    selectedThreadId &&
    threads.some((thread) => thread.rootEventId === selectedThreadId)
      ? selectedThreadId
      : (threads[0]?.rootEventId ?? null)

  if (!hydrated || conversationId == null || total === 0) return null

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
      case "embedded":
        return t("stateEmbedded")
      case "failed":
        return t("stateFailed")
      case "dismissed":
        return t("stateDismissed")
      case "pending":
        return t("stateStoreOnly")
    }
  }

  const replyState = (
    delivery: CollaborationDelivery,
    direction: "inbound" | "outbound"
  ) => {
    if (delivery.obligationState === "none") return null
    if (delivery.obligationState === "resolved") {
      if (!delivery.replyReceived) return t("noReplyNeeded")
      return direction === "inbound"
        ? t("stateReplied")
        : t("stateReplyReceived")
    }
    return direction === "inbound"
      ? t("stateNeedsReply")
      : t("stateAwaitingReply")
  }

  const mailStatusLabel = (delivery: CollaborationDelivery) => {
    const status: MailHumanStatus = mailHumanStatus(delivery)
    switch (status) {
      case "failed":
        return t("mailFailed")
      case "dismissed":
        return t("mailDismissed")
      case "unread":
        return t("mailUnread")
      case "read_awaiting":
        return t("mailReadAwaitingReply")
      case "replied":
        return t("mailReplied")
      case "read":
        return t("mailRead")
    }
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

  const peerName = (
    conversationIdValue: number,
    snapshotTitle: string | null | undefined
  ) =>
    participantLabel(
      conversationIdValue,
      snapshotTitle,
      conversationById.get(conversationIdValue)?.title,
      (values) => t("untitled", values)
    )

  const statusChip = (delivery: CollaborationDelivery) => {
    const status = mailHumanStatus(delivery)
    const tone = mailStatusVisual(status)
    return (
      <span
        data-mail-status={status}
        className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${tone.chip}`}
      >
        {mailStatusLabel(delivery)}
      </span>
    )
  }

  const deliveryActions = (delivery: CollaborationDelivery) => {
    const peerId =
      delivery.source.conversationId === conversationId
        ? delivery.target.conversationId
        : delivery.source.conversationId
    return (
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
  }

  const letterDetail = (
    delivery: CollaborationDelivery,
    direction: "inbound" | "outbound"
  ) => (
    <div className="space-y-1.5 border-t border-border/60 bg-muted/25 px-2.5 py-2">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="text-[11px] text-muted-foreground">
            <span>
              {direction === "inbound"
                ? t("from", {
                    name: peerName(
                      delivery.source.conversationId,
                      delivery.source.title
                    ),
                  })
                : t("to", {
                    name: peerName(
                      delivery.target.conversationId,
                      delivery.target.title
                    ),
                  })}
            </span>
          </p>
          {delivery.replyToEventId ? (
            <p className="mt-1 truncate text-[11px] text-muted-foreground">
              {t("replyToSubject", {
                subject:
                  letterSubjectLine(
                    [...feed.inbound, ...feed.outbound].find(
                      (item) => item.eventId === delivery.replyToEventId
                    )?.subject
                  ) || delivery.replyToEventId.slice(0, 8),
              })}
            </p>
          ) : null}
          <p className="mt-1 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
            {t("letterSubject")}
          </p>
          <p className="text-[13px] font-semibold leading-snug">
            {subjectOf(delivery, t("untitledSubject"))}
          </p>
          <div className="mt-1 flex flex-wrap items-center gap-1">
            {statusChip(delivery)}
            {replyState(delivery, direction) &&
            mailHumanStatus(delivery) !== "read_awaiting" &&
            mailHumanStatus(delivery) !== "replied" ? (
              <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                {replyState(delivery, direction)}
              </span>
            ) : null}
            {invocationState(delivery) &&
            delivery.state !== "embedded" &&
            delivery.state !== "pending" ? (
              <span className="text-[11px] text-muted-foreground">
                {invocationState(delivery)}
              </span>
            ) : null}
          </div>
        </div>
        {deliveryActions(delivery)}
      </div>
      {delivery.body ? (
        <p
          data-collaboration-letter-preview={delivery.id}
          className="rounded-md bg-background/80 px-2.5 py-2 text-[13px] leading-relaxed whitespace-pre-wrap break-words"
        >
          {delivery.body}
        </p>
      ) : null}
      {delivery.error ? (
        <p className="break-words text-[11px] text-destructive">
          {delivery.error}
        </p>
      ) : null}
      {delivery.interruptError ? (
        <p className="break-words text-[11px] text-destructive">
          {delivery.interruptError}
        </p>
      ) : null}
    </div>
  )

  const renderMailList = (
    items: CollaborationDelivery[],
    direction: "inbound" | "outbound",
    emptyLabel: string
  ) => {
    if (items.length === 0) {
      return (
        <p className="px-3 py-6 text-center text-xs text-muted-foreground">
          {emptyLabel}
        </p>
      )
    }
    return (
      <div className="max-h-[min(22rem,56vh)] overflow-y-auto">
        {items.map((delivery) => {
          const unread = isAgentUnread(delivery)
          const status = mailHumanStatus(delivery)
          const tone = mailStatusVisual(status)
          const peer =
            direction === "inbound"
              ? peerName(delivery.source.conversationId, delivery.source.title)
              : peerName(delivery.target.conversationId, delivery.target.title)
          const selectedRow = delivery.id === activeDeliveryId
          const snippet = letterBodySnippet(delivery.body, 56)
          const subject = subjectOf(delivery, t("untitledSubject"))
          return (
            <div
              key={delivery.id}
              className={`border-b border-border/50 last:border-b-0 ${
                selectedRow ? "bg-muted/60" : "hover:bg-muted/35"
              }`}
            >
              <button
                type="button"
                aria-current={selectedRow ? "true" : undefined}
                data-mail-status={status}
                className="flex w-full items-stretch text-left"
                onClick={() =>
                  setSelectedDeliveryId((current) =>
                    current === delivery.id ? null : delivery.id
                  )
                }
              >
                <span className={`w-0.5 shrink-0 ${tone.bar}`} />
                <span className="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5">
                  <span
                    className={`w-[5.5rem] shrink-0 truncate text-[11px] ${
                      unread
                        ? "font-semibold text-foreground"
                        : "text-muted-foreground"
                    }`}
                  >
                    {peer}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-[12px] leading-5">
                    <span className={unread ? "font-semibold" : "font-medium"}>
                      {subject}
                    </span>
                    {snippet ? (
                      <span className="text-muted-foreground">
                        {" — "}
                        {snippet}
                      </span>
                    ) : null}
                  </span>
                  {statusChip(delivery)}
                  <span className="w-12 shrink-0 text-right text-[10px] text-muted-foreground">
                    {formatMailTime(delivery.createdAt)}
                  </span>
                </span>
              </button>
              {selectedRow ? letterDetail(delivery, direction) : null}
            </div>
          )
        })}
      </div>
    )
  }

  const renderThreadWorkspace = (items: MailThread[]): ReactNode => {
    if (items.length === 0) {
      return (
        <p className="px-3 py-6 text-center text-xs text-muted-foreground">
          {t("threadsEmpty")}
        </p>
      )
    }
    return (
      <div className="max-h-[min(22rem,56vh)] overflow-y-auto">
        {items.map((thread) => {
          const latest = thread.items[thread.items.length - 1]
          const unread = thread.items.some(isAgentUnread)
          const selectedRow = thread.rootEventId === activeThreadId
          const latestStatus = latest ? mailHumanStatus(latest) : "read"
          const tone = mailStatusVisual(latestStatus)
          return (
            <div
              key={thread.rootEventId}
              className={`border-b border-border/50 last:border-b-0 ${
                selectedRow ? "bg-muted/60" : "hover:bg-muted/35"
              }`}
            >
              <button
                type="button"
                aria-current={selectedRow ? "true" : undefined}
                className="flex w-full items-stretch text-left"
                onClick={() =>
                  setSelectedThreadId((current) =>
                    current === thread.rootEventId ? null : thread.rootEventId
                  )
                }
              >
                <span className={`w-0.5 shrink-0 ${tone.bar}`} />
                <span className="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5">
                  <span
                    className={`min-w-0 flex-1 truncate text-[12px] ${
                      unread ? "font-semibold" : "font-medium"
                    }`}
                  >
                    {thread.subject}
                  </span>
                  <span className="shrink-0 text-[11px] text-muted-foreground">
                    {t("threadCount", { count: thread.items.length })}
                  </span>
                  {latest ? statusChip(latest) : null}
                </span>
              </button>
              {selectedRow ? (
                <div className="space-y-1.5 px-2 pb-2">
                  {thread.items.map((delivery) => {
                    const outbound =
                      delivery.source.conversationId === conversationId
                    return (
                      <article
                        key={delivery.id}
                        className="overflow-hidden rounded-md border border-border/70"
                      >
                        {letterDetail(
                          delivery,
                          outbound ? "outbound" : "inbound"
                        )}
                      </article>
                    )
                  })}
                </div>
              ) : null}
            </div>
          )
        })}
      </div>
    )
  }

  return (
    <section
      data-collaboration-banner=""
      className="border-b border-border/60 bg-background/80"
    >
      <Popover open={expanded} onOpenChange={setExpanded}>
        <div className="flex items-center px-3 py-1">
          <PopoverTrigger asChild>
            <button
              ref={triggerRef}
              type="button"
              className="inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-md px-1.5 py-0.5 text-left text-xs transition-colors hover:bg-muted/60"
            >
              <Mail className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              <span className="font-medium">{t("panelTitle")}</span>
              {inboxUnread > 0 ? (
                <span className="rounded-full bg-sky-500/15 px-1.5 py-px text-[10px] font-semibold text-sky-700 dark:text-sky-300">
                  {t("unreadCount", { count: inboxUnread })}
                </span>
              ) : null}
              {inboxAwaiting > 0 ? (
                <span className="rounded-full bg-amber-500/15 px-1.5 py-px text-[10px] font-semibold text-amber-800 dark:text-amber-300">
                  {t("awaitingReplyCount", { count: inboxAwaiting })}
                </span>
              ) : null}
              <span className="truncate text-muted-foreground">
                {t("inboxCount", { count: feed.inbound.length })}
                {feed.outbound.length > 0
                  ? ` · ${t("sentCount", { count: feed.outbound.length })}`
                  : ""}
              </span>
            </button>
          </PopoverTrigger>
        </div>
        <PopoverContent
          align="start"
          sideOffset={4}
          data-mailbox-panel=""
          className="w-[min(28rem,calc(100vw-1.25rem))] gap-0 overflow-hidden rounded-xl p-0"
        >
          <div role="tablist" className="flex gap-0.5 border-b bg-muted/40 p-1">
            {(
              [
                ["inbox", t("inboxLabel"), Inbox],
                ["sent", t("sentLabel"), Send],
                ["threads", t("threadsLabel"), Mail],
              ] as const
            ).map(([id, label, Icon]) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={view === id}
                className={`inline-flex flex-1 items-center justify-center gap-1 rounded-md px-2 py-1 text-[11px] font-medium ${
                  view === id
                    ? "bg-background text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground"
                }`}
                onClick={() => setView(id)}
              >
                <Icon className="h-3 w-3" aria-hidden="true" />
                {label}
              </button>
            ))}
          </div>
          {view === "inbox"
            ? renderMailList(feed.inbound, "inbound", t("inboxEmpty"))
            : null}
          {view === "sent"
            ? renderMailList(feed.outbound, "outbound", t("sentEmpty"))
            : null}
          {view === "threads" ? renderThreadWorkspace(threads) : null}
        </PopoverContent>
      </Popover>
    </section>
  )
}

interface SessionPendingContextBarProps {
  collaboration: UseCollaborationFeedReturn
}

export function SessionPendingContextBar({
  collaboration,
}: SessionPendingContextBarProps) {
  const t = useTranslations("Collaboration")
  const [expanded, setExpanded] = useState(false)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const conversationById = useMemo(
    () =>
      new Map(
        conversations.map((conversation) => [conversation.id, conversation])
      ),
    [conversations]
  )
  const { feed, hydrated, markSeen, dismiss } = collaboration
  const pending = useMemo(
    () =>
      feed.inbound.filter(
        (delivery) =>
          delivery.invocationPolicy === "store_only" &&
          delivery.state === "pending"
      ),
    [feed.inbound]
  )

  const pendingUnreadIds = useMemo(
    () =>
      pending
        .filter((delivery) => delivery.attentionState === "unread")
        .map((delivery) => delivery.id),
    [pending]
  )

  useEffect(() => {
    if (expanded && pendingUnreadIds.length > 0) {
      void markSeen(pendingUnreadIds)
    }
  }, [expanded, markSeen, pendingUnreadIds])

  if (!hydrated || pending.length === 0) return null

  return (
    <section className="mb-2 rounded-lg border border-amber-500/30 bg-amber-500/5 text-xs">
      <div className="flex items-center gap-2 px-3 py-2">
        <Inbox className="h-3.5 w-3.5 shrink-0 text-amber-700 dark:text-amber-400" />
        <span className="min-w-0 flex-1 font-medium">
          {t("pendingContextCount", { count: pending.length })}
        </span>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="h-6 shrink-0 px-2 text-[11px]"
          onClick={() => setExpanded((current) => !current)}
          aria-expanded={expanded}
        >
          {expanded ? t("pendingContextHide") : t("pendingContextReview")}
        </Button>
      </div>
      {expanded ? (
        <div className="max-h-40 divide-y overflow-y-auto border-t border-amber-500/20">
          {pending.map((delivery) => (
            <div key={delivery.id} className="flex items-start gap-2 px-3 py-2">
              <div className="min-w-0 flex-1">
                <p className="font-medium">
                  {t("from", {
                    name: participantLabel(
                      delivery.source.conversationId,
                      delivery.source.title,
                      conversationById.get(delivery.source.conversationId)
                        ?.title,
                      (values) => t("untitled", values)
                    ),
                  })}
                </p>
                <p className="mt-0.5 truncate font-medium">
                  {subjectOf(delivery, t("untitledSubject"))}
                </p>
                <p className="truncate text-muted-foreground">
                  {letterBodySnippet(delivery.body) ||
                    letterListPreview(delivery.subject, delivery.body)}
                </p>
              </div>
              <Button
                type="button"
                size="icon"
                variant="ghost"
                className="h-7 w-7 shrink-0"
                title={t("excludeFromNextTurn")}
                aria-label={t("excludeFromNextTurn")}
                onClick={() => void dismiss(delivery.id)}
              >
                <X className="h-3.5 w-3.5" />
              </Button>
            </div>
          ))}
        </div>
      ) : null}
    </section>
  )
}
