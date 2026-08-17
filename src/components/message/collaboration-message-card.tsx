"use client"

import { useMemo } from "react"

import { Message, MessageContent } from "@/components/ai-elements/message"
import type { CollaborationDelivery } from "@/lib/types"
import { CollapsibleUserMessage } from "./collapsible-user-message"
import { SessionMailFromBadge } from "./session-mail-peer-chip"

export { SessionMailFromBadge } from "./session-mail-peer-chip"

/** Pending inbound letter that is not a Harness user turn yet — same chrome. */
export function CollaborationMessageCard({
  delivery,
}: {
  delivery: CollaborationDelivery
  direction?: "inbound" | "outbound"
  currentConversationId?: number
}) {
  const bodyParts = useMemo(
    () => [{ type: "text" as const, text: delivery.body }],
    [delivery.body]
  )

  return (
    <article
      data-conversation-search-content
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
          />
          <div className="group/user-msg flex w-fit max-w-full items-start gap-1">
            <MessageContent data-collaboration-body>
              <CollapsibleUserMessage parts={bodyParts} />
            </MessageContent>
          </div>
        </div>
      </Message>
    </article>
  )
}
