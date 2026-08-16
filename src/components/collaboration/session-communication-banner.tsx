"use client"

import { useMemo, useState } from "react"
import {
  ArrowUpRight,
  ChevronDown,
  ChevronUp,
  Inbox,
  MessageSquareMore,
  Reply,
  RotateCcw,
  X,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { Button } from "@/components/ui/button"
import { useTabActions } from "@/contexts/tab-context"
import { useCollaborationFeed } from "@/hooks/use-collaboration-feed"
import { formatConversationTitle } from "@/lib/conversation-title"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import type { CollaborationDelivery } from "@/lib/types"
import { SessionMessageComposerDialog } from "./session-message-composer-dialog"

const COLLABORATION_DISABLED_REASON = "session_collaboration_disabled"

interface SessionCommunicationBannerProps {
  conversationId: number | null
}

function participantLabel(
  participant: CollaborationDelivery["source"],
  untitled: (values: { id: number }) => string
) {
  return (
    participant.title?.trim() || untitled({ id: participant.conversationId })
  )
}

export function SessionCommunicationBanner({
  conversationId,
}: SessionCommunicationBannerProps) {
  const t = useTranslations("Collaboration")
  const [expanded, setExpanded] = useState(false)
  const [replyingTo, setReplyingTo] = useState<CollaborationDelivery | null>(
    null
  )
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const conversationById = useMemo(
    () =>
      new Map(
        conversations.map((conversation) => [conversation.id, conversation])
      ),
    [conversations]
  )
  const { openTab } = useTabActions()
  const { feed, hydrated, markSeen, dismiss, retry } =
    useCollaborationFeed(conversationId)
  const total = feed.inbound.length + feed.outbound.length
  if (!hydrated || total === 0) return null

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
    if (!delivery.expectsReply) return null
    if (direction === "inbound") {
      return delivery.replyReceived ? t("stateReplied") : t("stateNeedsReply")
    }
    return delivery.replyReceived
      ? t("stateReplyReceived")
      : t("stateAwaitingReply")
  }

  const unreadIds = feed.inbound
    .filter(
      (delivery) => delivery.uiSeenAt == null && delivery.state !== "dismissed"
    )
    .map((delivery) => delivery.id)

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

  return (
    <section className="border-b border-border/60 bg-background/80 backdrop-blur-sm">
      <div className="mx-auto max-w-3xl px-4 py-1.5">
        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          className="flex w-full items-center gap-2 rounded-md px-1 py-1 text-left text-xs transition-colors hover:bg-muted/50"
        >
          <MessageSquareMore className="h-4 w-4 text-muted-foreground" />
          <span className="font-medium">{t("panelTitle")}</span>
          <span className="text-muted-foreground">
            {t("messageCount", { count: total })}
          </span>
          {feed.unreadCount > 0 ? (
            <span className="rounded-full bg-primary px-1.5 py-0.5 text-[10px] font-medium text-primary-foreground">
              {t("unreadCount", { count: feed.unreadCount })}
            </span>
          ) : null}
          <span className="ml-auto text-muted-foreground">
            {expanded ? (
              <ChevronUp className="h-4 w-4" />
            ) : (
              <ChevronDown className="h-4 w-4" />
            )}
          </span>
        </button>

        {expanded ? (
          <div className="space-y-3 pb-2 pt-1">
            {unreadIds.length > 0 ? (
              <div className="flex justify-end">
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-7 text-xs"
                  onClick={() => void markSeen(unreadIds)}
                >
                  {t("markAllRead")}
                </Button>
              </div>
            ) : null}

            {feed.inbound.length > 0 ? (
              <div className="space-y-1.5">
                <p className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                  <Inbox className="h-3.5 w-3.5" />
                  {t("inbound")}
                </p>
                {feed.inbound.map((delivery) => (
                  <article
                    key={delivery.id}
                    className="rounded-lg border bg-card/70 px-3 py-2 text-sm"
                  >
                    <div className="flex items-start gap-2">
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                          <span className="font-medium">
                            {t("from", {
                              name: participantLabel(
                                delivery.source,
                                (values) => t("untitled", values)
                              ),
                            })}
                          </span>
                          <span className="text-muted-foreground">
                            {delivery.source.agentType || t("unknownHarness")}
                          </span>
                          {delivery.uiSeenAt == null &&
                          delivery.state !== "dismissed" ? (
                            <span className="h-1.5 w-1.5 rounded-full bg-primary" />
                          ) : null}
                          {replyState(delivery, "inbound") ? (
                            <span
                              className={
                                delivery.replyReceived
                                  ? "rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
                                  : "rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-400"
                              }
                            >
                              {replyState(delivery, "inbound")}
                            </span>
                          ) : null}
                        </div>
                        <p className="mt-1 whitespace-pre-wrap break-words leading-relaxed">
                          {delivery.body}
                        </p>
                        <p className="mt-1 text-[11px] text-muted-foreground">
                          {invocationState(delivery)}
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
                      <div className="flex shrink-0 items-center">
                        {conversationById.has(
                          delivery.source.conversationId
                        ) ? (
                          <>
                            <Button
                              type="button"
                              size="icon"
                              variant="ghost"
                              className="h-7 w-7"
                              title={t("openSession")}
                              aria-label={t("openSession")}
                              onClick={() =>
                                openSession(delivery.source.conversationId)
                              }
                            >
                              <ArrowUpRight className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              type="button"
                              size="icon"
                              variant="ghost"
                              className="h-7 w-7"
                              title={t("reply")}
                              aria-label={t("reply")}
                              onClick={() => setReplyingTo(delivery)}
                            >
                              <Reply className="h-3.5 w-3.5" />
                            </Button>
                          </>
                        ) : null}
                        {(delivery.state === "failed" ||
                          (delivery.state === "queued" &&
                            delivery.queueState === "paused" &&
                            delivery.queuePausedReason !==
                              COLLABORATION_DISABLED_REASON)) &&
                        delivery.invocationPolicy === "invoke_when_idle" ? (
                          <Button
                            type="button"
                            size="icon"
                            variant="ghost"
                            className="h-7 w-7"
                            title={t("retry")}
                            aria-label={t("retry")}
                            onClick={() =>
                              void retry(delivery.queueItemId || delivery.id)
                            }
                          >
                            <RotateCcw className="h-3.5 w-3.5" />
                          </Button>
                        ) : null}
                        {delivery.state === "pending" ? (
                          <Button
                            type="button"
                            size="icon"
                            variant="ghost"
                            className="h-7 w-7"
                            title={t("dismiss")}
                            aria-label={t("dismiss")}
                            onClick={() => void dismiss(delivery.id)}
                          >
                            <X className="h-3.5 w-3.5" />
                          </Button>
                        ) : null}
                      </div>
                    </div>
                  </article>
                ))}
              </div>
            ) : null}

            {feed.outbound.length > 0 ? (
              <div className="space-y-1.5">
                <p className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                  {t("outbound")}
                </p>
                {feed.outbound.map((delivery) => (
                  <article
                    key={delivery.id}
                    className="rounded-lg border bg-muted/30 px-3 py-2 text-sm"
                  >
                    <div className="flex items-start gap-2">
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                          <span className="font-medium">
                            {t("to", {
                              name: participantLabel(
                                delivery.target,
                                (values) => t("untitled", values)
                              ),
                            })}
                          </span>
                          <span className="text-muted-foreground">
                            {delivery.state === "queued" ||
                            delivery.state === "embedding" ||
                            delivery.state === "embedded"
                              ? invocationState(delivery)
                              : delivery.state === "failed"
                                ? t("stateFailed")
                                : delivery.state === "dismissed"
                                  ? t("stateDismissedByTarget")
                                  : delivery.uiSeenAt
                                    ? t("stateSeen")
                                    : t("stateDelivered")}
                          </span>
                          {replyState(delivery, "outbound") ? (
                            <span
                              className={
                                delivery.replyReceived
                                  ? "rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
                                  : "rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-400"
                              }
                            >
                              {replyState(delivery, "outbound")}
                            </span>
                          ) : null}
                        </div>
                        <p className="mt-1 whitespace-pre-wrap break-words leading-relaxed">
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
                      {conversationById.has(delivery.target.conversationId) ? (
                        <Button
                          type="button"
                          size="icon"
                          variant="ghost"
                          className="h-7 w-7 shrink-0"
                          title={t("openSession")}
                          aria-label={t("openSession")}
                          onClick={() =>
                            openSession(delivery.target.conversationId)
                          }
                        >
                          <ArrowUpRight className="h-3.5 w-3.5" />
                        </Button>
                      ) : null}
                    </div>
                  </article>
                ))}
              </div>
            ) : null}
          </div>
        ) : null}
      </div>
      {replyingTo && conversationId != null ? (
        <SessionMessageComposerDialog
          key={replyingTo.eventId}
          sourceConversationId={conversationId}
          initialTargetConversationId={replyingTo.source.conversationId}
          replyToEventId={replyingTo.eventId}
          open
          onOpenChange={(open) => {
            if (!open) setReplyingTo(null)
          }}
        />
      ) : null}
    </section>
  )
}
