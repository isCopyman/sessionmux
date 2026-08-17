"use client"

import { parseSessionReadMessageOutput } from "@/lib/session-read-message-tool"
import { mailHumanStatus } from "@/lib/mail-human-status"
import { SessionMailCard } from "./session-mail-card"
import { useMailDelivery } from "./session-mail-lookup"

export function SessionReadMessageCard({
  letterKey,
  output,
}: {
  letterKey: string
  output: string | null
}) {
  const letter = parseSessionReadMessageOutput(output)
  if (!letter) return null
  return <SessionReadMessageCardView letterKey={letterKey} letter={letter} />
}

function SessionReadMessageCardView({
  letterKey,
  letter,
}: {
  letterKey: string
  letter: NonNullable<ReturnType<typeof parseSessionReadMessageOutput>>
}) {
  const live = useMailDelivery(letter.eventId)
  return (
    <div data-session-read-message="">
      <SessionMailCard
        direction="inbound"
        eventId={letter.eventId || live?.eventId}
        fromConversationId={letter.fromSessionId ?? live?.source.conversationId}
        fromTitle={letter.fromTitle ?? live?.source.title}
        fromAgentType={live?.source.agentType}
        subject={letter.title || live?.subject}
        body={letter.body}
        replyToEventId={live?.replyToEventId}
        status={
          live
            ? mailHumanStatus(live)
            : letter.expectsReply
              ? "read_awaiting"
              : "read"
        }
        expectsReply={live?.expectsReply ?? letter.expectsReply}
        letterKey={letterKey}
        action="opened"
      />
    </div>
  )
}
