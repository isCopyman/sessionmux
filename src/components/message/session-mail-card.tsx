"use client"

import { CornerDownRight, Inbox, Mail, Send, Users } from "lucide-react"
import { useTranslations } from "next-intl"

import { cn } from "@/lib/utils"
import { letterSubjectLine, letterListPreview } from "@/lib/conversation-title"
import {
  mailHumanStatus,
  mailNoReplyNeeded,
  mailStatusLabelKey,
  mailStatusVisual,
  type MailDirection,
  type MailHumanStatus,
} from "@/lib/mail-human-status"
import { useSessionLetterUiStore } from "@/stores/session-letter-ui-store"
import { SessionLetterActions } from "./session-letter-render-toggle"
import { SessionLetterBody } from "./session-letter-body"
import { SessionMailPeerChip } from "./session-mail-peer-chip"
import { SessionRoomChip } from "./session-room-chip"
import { useMailDelivery } from "./session-mail-lookup"

export function SessionMailStatusChip({
  status,
  direction = "inbound",
}: {
  status: MailHumanStatus
  direction?: MailDirection
}) {
  const t = useTranslations("Collaboration")
  const tone = mailStatusVisual(status)
  return (
    <span
      data-mail-status={status}
      className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${tone.chip}`}
    >
      {t(mailStatusLabelKey(status, direction))}
    </span>
  )
}

export function SessionMailNoReplyChip() {
  const t = useTranslations("Collaboration")
  return (
    <span
      data-mail-no-reply=""
      className="rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground"
    >
      {t("noReplyNeeded")}
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
  expectsReply,
  letterKey,
  action,
  channel = "mailbox",
  roomId,
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
  expectsReply?: boolean | null
  letterKey?: string
  action?: "opened" | "sent"
  channel?: "mailbox" | "room"
  roomId?: string | null
}) {
  const t = useTranslations("Collaboration")
  const live = useMailDelivery(eventId)
  const parent = useMailDelivery(replyToEventId ?? live?.replyToEventId)
  const requestFocus = useSessionLetterUiStore((state) => state.requestFocus)
  const resolvedStatus = status ?? (live ? mailHumanStatus(live) : null)
  const tone = resolvedStatus ? mailStatusVisual(resolvedStatus) : null
  // FYI badge: known-false expects_reply, or a duty later waived by the peer.
  const noReplyNeeded = live ? mailNoReplyNeeded(live) : expectsReply === false
  const parentSubject =
    letterSubjectLine(parent?.subject) ||
    letterListPreview(parent?.subject, parent?.body, 32)
  const replyId = replyToEventId ?? live?.replyToEventId ?? null

  const isRoom = channel === "room"
  const DirectionIcon = isRoom
    ? Users
    : direction === "outbound"
      ? Send
      : direction === "inbound"
        ? Inbox
        : Mail

  return (
    <article
      data-session-mail-card={direction}
      data-letter-event-id={eventId ?? undefined}
      data-collaboration-channel={channel}
      className={cn(
        // Letters live on the agent's side of the timeline: the right column
        // stays reserved for what the human typed. Direction is carried by the
        // card itself (icon, tint, 来自/发给), not by alignment.
        "group/letter w-full max-w-[36rem] overflow-hidden rounded-lg border",
        direction === "inbound" &&
          (isRoom
            ? "self-start border-violet-500/30 bg-violet-500/[0.05]"
            : "self-start border-sky-500/30 bg-sky-500/[0.05]"),
        direction === "outbound" && "self-start border-border bg-muted/55",
        direction === "system" &&
          "mx-auto self-center border-dashed bg-muted/25"
      )}
    >
      <div className="flex">
        <span className={`w-0.5 shrink-0 ${tone?.bar ?? "bg-border"}`} />
        <div className="min-w-0 flex-1 px-3 py-2">
          <header className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
            <DirectionIcon className="h-3 w-3 shrink-0" aria-hidden="true" />
            {isRoom ? <SessionRoomChip roomId={roomId} /> : null}
            {direction === "system" ? (
              <span className="font-medium text-foreground">
                {t("fromMailSystem")}
              </span>
            ) : null}
            {action === "opened" ? (
              <span className="font-medium text-sky-800 dark:text-sky-300">
                {t("openedLetter")}
              </span>
            ) : null}
            {action === "sent" ? (
              <span className="font-medium text-foreground">
                {t("sentLetter")}
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
              <SessionMailStatusChip
                status={resolvedStatus}
                direction={direction}
              />
            ) : null}
            {noReplyNeeded ? <SessionMailNoReplyChip /> : null}
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
