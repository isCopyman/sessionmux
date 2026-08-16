"use client"

import { useMemo } from "react"
import { ArrowUpRight, MessageSquareMore } from "lucide-react"
import { useTranslations } from "next-intl"

import { MessageContent } from "@/components/ai-elements/message"
import { Button } from "@/components/ui/button"
import { useTabActions } from "@/contexts/tab-context"
import { formatConversationTitle } from "@/lib/conversation-title"
import type { CollaborationDelivery } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { ContentPartsRenderer } from "./content-parts-renderer"

export function CollaborationMessageCard({
  delivery,
}: {
  delivery: CollaborationDelivery
  currentConversationId?: number
}) {
  const t = useTranslations("Collaboration")
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const sourceConversation = conversations.find(
    (conversation) => conversation.id === delivery.source.conversationId
  )
  const { openTab } = useTabActions()
  const sourceName = `Session ${delivery.source.conversationId}`
  const bodyParts = useMemo(
    () => [{ type: "text" as const, text: delivery.body }],
    [delivery.body]
  )

  const openSource = () => {
    if (!sourceConversation) return
    openTab(
      sourceConversation.folder_id,
      sourceConversation.id,
      sourceConversation.agent_type,
      true,
      formatConversationTitle(sourceConversation.title) || undefined
    )
  }

  return (
      <article
        className="w-full rounded-lg border border-primary/20 bg-primary/[0.035] px-4 py-3"
        data-conversation-search-content
        data-collaboration-event-id={delivery.eventId}
        data-collaboration-delivery-id={delivery.id}
        data-embedded-turn-ref={delivery.embeddedTurnRef ?? undefined}
      >
        <header className="flex min-w-0 items-start gap-2">
          <MessageSquareMore className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
              <span className="truncate font-medium">
                {t("transcriptFrom", { name: sourceName })}
              </span>
              <span className="truncate text-muted-foreground">
                {delivery.source.agentType || t("unknownHarness")}
                {delivery.source.title?.trim()
                  ? ` · ${formatConversationTitle(delivery.source.title)}`
                  : ""}
              </span>
            </div>
          </div>
          <div className="flex shrink-0 items-center">
            <Button
              type="button"
              size="icon"
              variant="ghost"
              className="h-7 w-7"
              title={t("openSession")}
              aria-label={t("openSession")}
              disabled={!sourceConversation}
              onClick={openSource}
            >
              <ArrowUpRight className="h-3.5 w-3.5" />
            </Button>
          </div>
        </header>
        <MessageContent className="mt-2">
          <ContentPartsRenderer parts={bodyParts} role="assistant" />
        </MessageContent>
      </article>
  )
}
