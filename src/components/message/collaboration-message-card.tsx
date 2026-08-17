"use client"

import { Message, MessageContent } from "@/components/ai-elements/message"
import type { CollaborationDelivery } from "@/lib/types"
import { SessionLetterBody } from "./session-letter-body"
import { SessionMailFromBadge } from "./session-mail-peer-chip"

export {
  SessionMailFromBadge,
  SessionMailSystemBadge,
} from "./session-mail-peer-chip"

/** Pending inbound letter that is not a Harness user turn yet — same chrome. */
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
      data-letter-event-id={delivery.eventId}
      data-collaboration-event-id={delivery.eventId}
      data-collaboration-delivery-id={delivery.id}
      data-collaboration-direction="inbound"
      data-embedded-turn-ref={delivery.embeddedTurnRef ?? undefined}
    >
      <Message from="user">
        <div className="flex w-fit max-w-full flex-col items-end self-end">
          <SessionMailFromBadge
            conversationId={delivery.source.conversationId}
            title={delivery.source.title}
            agentType={delivery.source.agentType}
            eventId={delivery.eventId}
          />
          <div className="group/user-msg flex w-fit max-w-full items-start gap-1">
            <MessageContent data-collaboration-body>
              <SessionLetterBody
                subject={delivery.subject}
                body={delivery.body}
              />
            </MessageContent>
          </div>
        </div>
      </Message>
    </article>
  )
}
