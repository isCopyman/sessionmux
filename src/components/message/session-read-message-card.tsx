"use client"

import { useTranslations } from "next-intl"

import { Message, MessageContent } from "@/components/ai-elements/message"
import { parseSessionReadMessageOutput } from "@/lib/session-read-message-tool"
import { SessionLetterActions } from "./session-letter-render-toggle"
import { SessionLetterBody } from "./session-letter-body"
import { SessionMailFromBadge } from "./session-mail-peer-chip"

export function SessionReadMessageCard({
  letterKey,
  output,
}: {
  letterKey: string
  output: string | null
}) {
  const t = useTranslations("Collaboration")
  const letter = parseSessionReadMessageOutput(output)
  if (!letter) return null

  return (
    <div
      className="group/letter flex w-fit max-w-full flex-col items-end self-end"
      data-session-read-message=""
      data-letter-event-id={letter.eventId || undefined}
    >
      {letter.fromSessionId != null ? (
        <SessionMailFromBadge
          conversationId={letter.fromSessionId}
          title={letter.fromTitle}
          eventId={letter.eventId || null}
        />
      ) : (
        <div className="mb-1 text-[0.6875rem] text-muted-foreground">
          {t("openedLetter")}
        </div>
      )}
      <div className="group/user-msg flex w-fit max-w-full items-start gap-1">
        <Message from="user">
          <MessageContent data-collaboration-body>
            <SessionLetterBody subject={letter.title} body={letter.body} />
            {letter.expectsReply ? (
              <p className="mt-1 text-[11px] text-amber-700 dark:text-amber-400">
                {t("mailReadAwaitingReply")}
              </p>
            ) : null}
          </MessageContent>
        </Message>
        <SessionLetterActions letterKey={letterKey} copyText={letter.body} />
      </div>
    </div>
  )
}
