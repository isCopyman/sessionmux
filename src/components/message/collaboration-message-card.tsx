"use client"

import type { CollaborationDelivery } from "@/lib/types"
import { mailHumanStatus } from "@/lib/mail-human-status"
import { SessionMailCard } from "./session-mail-card"

export {
  SessionMailFromBadge,
  SessionMailSystemBadge,
} from "./session-mail-peer-chip"

/** Pending inbound letter that is not a Harness user turn yet. */
export function CollaborationMessageCard({
  delivery,
}: {
  delivery: CollaborationDelivery
  direction?: "inbound" | "outbound"
  currentConversationId?: number
}) {
  return (
    <article
      data-conversation-search-content
      data-collaboration-event-id={delivery.eventId}
      data-collaboration-delivery-id={delivery.id}
      data-collaboration-direction="inbound"
      data-embedded-turn-ref={delivery.embeddedTurnRef ?? undefined}
    >
      <SessionMailCard
        direction="inbound"
        eventId={delivery.eventId}
        fromConversationId={delivery.source.conversationId}
        fromTitle={delivery.source.title}
        fromAgentType={delivery.source.agentType}
        subject={delivery.subject}
        body={delivery.body}
        replyToEventId={delivery.replyToEventId}
        status={mailHumanStatus(delivery)}
        expectsReply={delivery.expectsReply}
      />
    </article>
  )
}
