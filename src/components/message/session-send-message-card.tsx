"use client"

import {
  parseSessionSendMessageEventId,
  parseSessionSendMessageInput,
} from "@/lib/session-send-message-tool"
import { mailHumanStatus } from "@/lib/mail-human-status"
import { SessionMailCard } from "./session-mail-card"
import { useMailDelivery } from "./session-mail-lookup"

export function SessionSendMessageCard({
  letterKey,
  input,
  output = null,
}: {
  letterKey: string
  input: string | null
  output?: string | null
}) {
  const parsed = parseSessionSendMessageInput(input)
  const eventId = parseSessionSendMessageEventId(output)
  const live = useMailDelivery(eventId)
  if (!parsed || parsed.targetSessionIds.length === 0) return null

  return (
    <div data-session-send-message="">
      <SessionMailCard
        direction="outbound"
        eventId={eventId}
        toConversationIds={parsed.targetSessionIds}
        subject={parsed.title || live?.subject}
        body={parsed.content}
        replyToEventId={live?.replyToEventId}
        status={live ? mailHumanStatus(live) : null}
        expectsReply={live?.expectsReply ?? parsed.expectsReply}
        letterKey={letterKey}
        action="sent"
      />
    </div>
  )
}
