"use client"

import { CornerDownRight, Mail } from "lucide-react"
import { useTranslations } from "next-intl"

import { cn } from "@/lib/utils"
import { letterSubjectLine, letterListPreview } from "@/lib/conversation-title"
import {
  mailHumanStatus,
  mailStatusVisual,
  type MailHumanStatus,
} from "@/lib/mail-human-status"
import { useSessionLetterUiStore } from "@/stores/session-letter-ui-store"
import { SessionLetterActions } from "./session-letter-render-toggle"
import { SessionLetterBody } from "./session-letter-body"
import { SessionMailPeerChip } from "./session-mail-peer-chip"
import { useMailDelivery } from "./session-mail-lookup"

export function SessionMailStatusChip({ status }: { status: MailHumanStatus }) {
  const t = useTranslations("Collaboration")
  const tone = mailStatusVisual(status)
  const label =
    status === "failed"
      ? t("mailFailed")
      : status === "dismissed"
        ? t("mailDismissed")
        : status === "unread"
          ? t("mailUnread")
          : status === "read_awaiting"
            ? t("mailReadAwaitingReply")
            : status === "replied"
              ? t("mailReplied")
              : t("mailRead")
  return (
    <span
      data-mail-status={status}
      className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${tone.chip}`}
    >
      {label}
    </span>
  )
}

export function SessionMailCard({
  direction,
  eventId,
  fromConversationId,
  fromTitle,
  fromAgentType,
  toConversationIds,
  subject,
  body,
  replyToEventId,
  status,
  letterKey,
}: {
  direction: "inbound" | "outbound" | "system"
  eventId?: string | null
  fromConversationId?: number | null
  fromTitle?: string | null
  fromAgentType?: string | null
  toConversationIds?: number[]
  subject?: string | null
  body: string
  replyToEventId?: string | null
  status?: MailHumanStatus | null
  letterKey?: string
}) {
  const t = useTranslations("Collaboration")
  const live = useMailDelivery(eventId)
  const parent = useMailDelivery(replyToEventId ?? live?.replyToEventId)
  const requestFocus = useSessionLetterUiStore((state) => state.requestFocus)
  const resolvedStatus = status ?? (live ? mailHumanStatus(live) : null)
  const tone = resolvedStatus ? mailStatusVisual(resolvedStatus) : null
  const parentSubject =
    letterSubjectLine(parent?.subject) ||
    letterListPreview(parent?.subject, parent?.body, 32)
  const replyId = replyToEventId ?? live?.replyToEventId ?? null

  return (
    <article
      data-session-mail-card={direction}
      data-letter-event-id={eventId ?? undefined}
      className={cn(
        "group/letter w-full max-w-[36rem] overflow-hidden rounded-lg border bg-card/80",
        direction === "inbound" && "self-start",
        direction === "outbound" && "self-start",
        direction === "system" && "border-dashed"
      )}
    >
      <div className="flex">
        <span className={`w-0.5 shrink-0 ${tone?.bar ?? "bg-border"}`} />
        <div className="min-w-0 flex-1 px-3 py-2">
          <header className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
            <Mail className="h-3 w-3 shrink-0" aria-hidden="true" />
            {direction === "system" ? (
              <span className="font-medium text-foreground">
                {t("fromMailSystem")}
              </span>
            ) : null}
            {direction === "inbound" && fromConversationId != null ? (
              <SessionMailPeerChip
                conversationId={fromConversationId}
                title={fromTitle}
                agentType={fromAgentType}
                eventId={eventId}
                prefix={t("fromPrefix")}
              />
            ) : null}
            {direction === "outbound" && toConversationIds
              ? toConversationIds.map((id, index) => (
                  <SessionMailPeerChip
                    key={id}
                    conversationId={id}
                    eventId={eventId}
                    prefix={index === 0 ? t("toPrefix") : ""}
                  />
                ))
              : null}
            {resolvedStatus ? (
              <SessionMailStatusChip status={resolvedStatus} />
            ) : null}
            {letterKey ? (
              <SessionLetterActions
                letterKey={letterKey}
                copyText={body}
                className="ml-auto opacity-100"
              />
            ) : null}
          </header>
          {replyId ? (
            <button
              type="button"
              className="mt-1 inline-flex max-w-full items-center gap-1 truncate text-left text-[11px] text-muted-foreground hover:text-foreground"
              onClick={() => {
                const peer = parent?.source.conversationId ?? fromConversationId
                if (peer != null) requestFocus(peer, replyId)
              }}
            >
              <CornerDownRight className="h-3 w-3 shrink-0" />
              <span className="truncate">
                {t("replyToSubject", {
                  subject: parentSubject || replyId.slice(0, 8),
                })}
              </span>
            </button>
          ) : null}
          <div className="mt-1.5">
            <SessionLetterBody subject={subject} body={body} />
          </div>
        </div>
      </div>
    </article>
  )
}
