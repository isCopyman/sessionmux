"use client"

import { useTranslations } from "next-intl"

import { cn } from "@/lib/utils"
import {
  parseSessionSendMessageEventId,
  parseSessionSendMessageInput,
} from "@/lib/session-send-message-tool"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useSessionLetterUiStore } from "@/stores/session-letter-ui-store"
import { SessionLetterBody } from "./session-letter-body"
import { SessionLetterActions } from "./session-letter-render-toggle"
import { SessionMailPeerChip } from "./session-mail-peer-chip"

export function SessionSendMessageCard({
  letterKey,
  input,
  output = null,
}: {
  letterKey: string
  input: string | null
  output?: string | null
}) {
  const t = useTranslations("Collaboration")
  const focusedEventId = useSessionLetterUiStore(
    (state) => state.focusedEventId
  )
  const parsed = parseSessionSendMessageInput(input)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const targetIds = parsed?.targetSessionIds ?? []
  const content = parsed?.content ?? ""
  const subject = parsed?.title ?? ""
  const eventId = parseSessionSendMessageEventId(output)

  if (!parsed || targetIds.length === 0) return null

  return (
    <div
      className="group/letter flex w-fit max-w-[40rem] items-end gap-1"
      data-session-send-message=""
      data-letter-event-id={eventId ?? undefined}
      data-target-session-id={targetIds[0]}
    >
      <article
        className={cn(
          "min-w-0 rounded-lg border border-border bg-muted/30 px-3 py-2.5",
          eventId && focusedEventId === eventId && "ring-2 ring-primary/35"
        )}
      >
        <header className="mb-1.5 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          {targetIds.map((targetId, index) => {
            const target = conversations.find(
              (conversation) => conversation.id === targetId
            )
            return (
              <SessionMailPeerChip
                key={targetId}
                conversationId={targetId}
                title={target?.title}
                agentType={target?.agent_type}
                eventId={eventId}
                prefix={index === 0 ? t("toPrefix") : ""}
              />
            )
          })}
        </header>
        <SessionLetterBody subject={subject} body={content} />
      </article>
      <SessionLetterActions letterKey={letterKey} copyText={content} />
    </div>
  )
}
