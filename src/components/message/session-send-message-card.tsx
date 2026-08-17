"use client"

import { useMemo } from "react"
import { useTranslations } from "next-intl"

import {
  parseSessionSendMessageEventId,
  parseSessionSendMessageInput,
} from "@/lib/session-send-message-tool"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { ContentPartsRenderer } from "./content-parts-renderer"
import { SessionMailPeerChip } from "./session-mail-peer-chip"

export function SessionSendMessageCard({
  input,
  output = null,
}: {
  input: string | null
  output?: string | null
}) {
  const t = useTranslations("Collaboration")
  const parsed = parseSessionSendMessageInput(input)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const targetIds = parsed?.targetSessionIds ?? []
  const bodyParts = useMemo(
    () => [{ type: "text" as const, text: parsed?.content ?? "" }],
    [parsed?.content]
  )

  if (!parsed || targetIds.length === 0) return null

  const eventId = parseSessionSendMessageEventId(output)

  return (
    <article
      className="w-full max-w-[min(40rem,88%)] rounded-lg border border-border bg-muted/30 px-3 py-2.5"
      data-session-send-message=""
      data-target-session-id={targetIds[0]}
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
      <div className="text-sm">
        <ContentPartsRenderer parts={bodyParts} role="assistant" />
      </div>
    </article>
  )
}
