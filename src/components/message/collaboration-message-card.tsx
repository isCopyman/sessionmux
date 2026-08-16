"use client"

import { useMemo, useState } from "react"
import { ArrowUpRight, Bot, Eye, MessageSquareMore, Reply } from "lucide-react"
import { useTranslations } from "next-intl"

import { MessageContent } from "@/components/ai-elements/message"
import { SessionMessageComposerDialog } from "@/components/collaboration/session-message-composer-dialog"
import { Button } from "@/components/ui/button"
import { useTabActions } from "@/contexts/tab-context"
import { formatConversationTitle } from "@/lib/conversation-title"
import type { CollaborationDelivery } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { ContentPartsRenderer } from "./content-parts-renderer"

function statusClass(active: boolean) {
  return active
    ? "rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-400"
    : "rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
}

export function CollaborationMessageCard({
  delivery,
  currentConversationId,
}: {
  delivery: CollaborationDelivery
  currentConversationId: number
}) {
  const t = useTranslations("Collaboration")
  const [replyOpen, setReplyOpen] = useState(false)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const sourceConversation = conversations.find(
    (conversation) => conversation.id === delivery.source.conversationId
  )
  const { openTab } = useTabActions()
  const sourceName =
    delivery.source.title?.trim() ||
    t("untitled", { id: delivery.source.conversationId })
  const bodyParts = useMemo(
    () => [{ type: "text" as const, text: delivery.body }],
    [delivery.body]
  )
  const replyState =
    delivery.obligationState === "awaiting_reply"
      ? t("stateNeedsReply")
      : delivery.obligationState === "resolved"
        ? delivery.replyReceived
          ? t("stateReplied")
          : t("noReplyNeeded")
        : null

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
    <>
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
                {delivery.source.folderPath
                  ? ` · ${delivery.source.folderPath}`
                  : ""}
              </span>
            </div>
            <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
              <span
                className={statusClass(delivery.attentionState === "unread")}
              >
                <Eye className="mr-1 inline h-3 w-3" />
                {delivery.attentionState === "opened"
                  ? t("stateSeen")
                  : t("unreadCount", { count: 1 })}
              </span>
              <span className={statusClass(false)}>
                <Bot className="mr-1 inline h-3 w-3" />
                {t("stateEmbedded")}
              </span>
              {replyState ? (
                <span
                  className={statusClass(
                    delivery.obligationState === "awaiting_reply"
                  )}
                >
                  {replyState}
                </span>
              ) : null}
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
            <Button
              type="button"
              size="icon"
              variant="ghost"
              className="h-7 w-7"
              title={t("reply")}
              aria-label={t("reply")}
              disabled={!sourceConversation}
              onClick={() => setReplyOpen(true)}
            >
              <Reply className="h-3.5 w-3.5" />
            </Button>
          </div>
        </header>
        <MessageContent className="mt-2">
          <ContentPartsRenderer parts={bodyParts} role="assistant" />
        </MessageContent>
      </article>
      <SessionMessageComposerDialog
        sourceConversationId={currentConversationId}
        open={replyOpen}
        onOpenChange={setReplyOpen}
        initialTargetConversationId={delivery.source.conversationId}
        replyToEventId={delivery.eventId}
      />
    </>
  )
}
