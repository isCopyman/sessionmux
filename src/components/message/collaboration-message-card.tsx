"use client"

import { useMemo } from "react"
import { ArrowUpRight } from "lucide-react"
import { useTranslations } from "next-intl"

import { Message, MessageContent } from "@/components/ai-elements/message"
import { Button } from "@/components/ui/button"
import { useTabActions } from "@/contexts/tab-context"
import {
  formatConversationTitle,
  formatSessionMailName,
} from "@/lib/conversation-title"
import type { CollaborationDelivery } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { CollapsibleUserMessage } from "./collapsible-user-message"

export function SessionMailFromBadge({
  conversationId,
  title,
}: {
  conversationId: number
  title?: string | null
  agentType?: string | null
}) {
  const t = useTranslations("Collaboration")
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const peerConversation = conversations.find(
    (conversation) => conversation.id === conversationId
  )
  const { openTab } = useTabActions()
  const name = formatSessionMailName(
    peerConversation?.title ?? title,
    conversationId
  )

  return (
    <div className="mb-1 flex w-fit max-w-full items-center gap-1 self-end text-[0.6875rem] text-muted-foreground">
      <span className="truncate" title={peerConversation?.title ?? title ?? name}>
        {t("fromSession", { name })}
      </span>
      <Button
        type="button"
        size="icon"
        variant="ghost"
        className="h-5 w-5 shrink-0"
        title={t("openSession")}
        aria-label={t("openSession")}
        disabled={!peerConversation}
        onClick={() => {
          if (!peerConversation) return
          openTab(
            peerConversation.folder_id,
            peerConversation.id,
            peerConversation.agent_type,
            true,
            formatConversationTitle(peerConversation.title) || undefined
          )
        }}
      >
        <ArrowUpRight className="h-3 w-3" />
      </Button>
    </div>
  )
}

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
