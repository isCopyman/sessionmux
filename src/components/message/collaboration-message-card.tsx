"use client"

import { MessageSquareMore } from "lucide-react"
import { useTranslations } from "next-intl"

import type { CollaborationMessageEnvelope } from "./collaboration-message-envelope"

export function CollaborationMessageCard({
  envelope,
}: {
  envelope: CollaborationMessageEnvelope
}) {
  const t = useTranslations("Collaboration")
  const sourceName =
    envelope.sourceTitle?.trim() ||
    t("untitled", { id: envelope.sourceConversationId })

  return (
    <article
      className="w-full rounded-lg border border-primary/20 bg-primary/[0.035] px-4 py-3"
      data-conversation-search-content
      data-collaboration-event-id={envelope.eventId}
    >
      <header className="flex min-w-0 items-center gap-2 text-xs">
        <MessageSquareMore className="h-4 w-4 shrink-0 text-primary" />
        <span className="truncate font-medium">
          {t("transcriptFrom", { name: sourceName })}
        </span>
        <span className="truncate text-muted-foreground">
          {envelope.sourceAgentType}
          {envelope.sourceFolderPath ? ` · ${envelope.sourceFolderPath}` : ""}
        </span>
      </header>
      <p className="mt-2 whitespace-pre-wrap break-words text-sm leading-relaxed">
        {envelope.body}
      </p>
    </article>
  )
}
