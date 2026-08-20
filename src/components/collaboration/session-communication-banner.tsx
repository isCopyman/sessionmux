"use client"

import { useEffect, useMemo, useState } from "react"
import { Inbox, Mail, X } from "lucide-react"
import { useTranslations } from "next-intl"
import { Button } from "@/components/ui/button"
import {
  useCollaborationFeed,
  type UseCollaborationFeedReturn,
} from "@/hooks/use-collaboration-feed"
import {
  letterBodySnippet,
  letterListPreview,
  letterSubjectLine,
  resolveLiveSessionTitle,
} from "@/lib/conversation-title"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import type { CollaborationDelivery } from "@/lib/types"
import { isAgentUnread } from "@/lib/mail-human-status"
import { SessionMailboxDialog } from "./session-mailbox-dialog"

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
  const { feed, hydrated } = collaboration
  const total = feed.inbound.length + feed.outbound.length
  const inboxUnread = feed.inbound.filter(isAgentUnread).length
  const inboxAwaiting = feed.inbound.filter(
    (delivery) => delivery.obligationState === "awaiting_reply"
  ).length

  if (!hydrated || conversationId == null || total === 0) return null

  return (
    <section
      data-collaboration-banner=""
      className="border-b border-border/60 bg-background/80"
    >
      <div className="flex items-center px-3 py-1">
        <button
          type="button"
          className="inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-md px-1.5 py-0.5 text-left text-xs transition-colors hover:bg-muted/60"
          onClick={() => setExpanded(true)}
        >
          <Mail className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          <span className="font-medium">{t("panelTitle")}</span>
          {inboxUnread > 0 ? (
            <span className="rounded-full bg-sky-500/15 px-1.5 py-px text-[10px] font-semibold text-sky-700 dark:text-sky-300">
              {t("unreadCount", { count: inboxUnread })}
            </span>
          ) : null}
          {inboxAwaiting > 0 ? (
            // Inbound only: these are letters this Session has to answer, not
            // ones it is waiting on. The hover text says so.
            <span
              title={t("needsReplyHint")}
              className="rounded-full bg-amber-500/15 px-1.5 py-px text-[10px] font-semibold text-amber-800 dark:text-amber-300"
            >
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
      </div>
      <SessionMailboxDialog
        open={expanded}
        onOpenChange={setExpanded}
        conversationId={conversationId}
        collaboration={collaboration}
      />
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
