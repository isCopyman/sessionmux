"use client"

import { useMemo } from "react"
import { ArrowDownLeft, ArrowUpRight } from "lucide-react"
import { useTranslations } from "next-intl"

import { Button } from "@/components/ui/button"
import { useTabActions } from "@/contexts/tab-context"
import { formatConversationTitle } from "@/lib/conversation-title"
import type { CollaborationDelivery } from "@/lib/types"
import { cn } from "@/lib/utils"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { ContentPartsRenderer } from "./content-parts-renderer"

function formatSentAt(iso: string): string | null {
  const ms = Date.parse(iso)
  if (Number.isNaN(ms)) return null
  return new Date(ms).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  })
}

export function CollaborationMessageCard({
  delivery,
  direction = "inbound",
}: {
  delivery: CollaborationDelivery
  direction?: "inbound" | "outbound"
  currentConversationId?: number
}) {
  const t = useTranslations("Collaboration")
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const peer = direction === "outbound" ? delivery.target : delivery.source
  const peerConversation = conversations.find(
    (conversation) => conversation.id === peer.conversationId
  )
  const { openTab } = useTabActions()
  const rawTitle = formatConversationTitle(
    peerConversation?.title ?? peer.title
  )
  const peerTitle =
    rawTitle && rawTitle.length <= 36
      ? rawTitle
      : `Session ${peer.conversationId}`
  const bodyParts = useMemo(
    () => [{ type: "text" as const, text: delivery.body }],
    [delivery.body]
  )
  const outbound = direction === "outbound"
  const sentAt = formatSentAt(delivery.createdAt)

  const openPeer = () => {
    if (!peerConversation) return
    openTab(
      peerConversation.folder_id,
      peerConversation.id,
      peerConversation.agent_type,
      true,
      formatConversationTitle(peerConversation.title) || undefined
    )
  }

  return (
    <article
      className={cn(
        "my-1 w-full max-w-[min(40rem,88%)] rounded-xl border px-4 py-3",
        outbound
          ? "ml-auto border-sky-500/30 bg-sky-500/[0.07]"
          : "border-primary/20 bg-primary/[0.045]"
      )}
      data-conversation-search-content
      data-collaboration-event-id={delivery.eventId}
      data-collaboration-delivery-id={delivery.id}
      data-collaboration-direction={direction}
      data-embedded-turn-ref={delivery.embeddedTurnRef ?? undefined}
    >
      <header className="flex min-w-0 items-start gap-2">
        {outbound ? (
          <ArrowUpRight className="mt-0.5 h-4 w-4 shrink-0 text-sky-600 dark:text-sky-400" />
        ) : (
          <ArrowDownLeft className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
        )}
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span
              className={cn(
                "rounded-full px-1.5 py-0.5 text-[0.625rem] font-medium leading-none",
                outbound
                  ? "bg-sky-500/15 text-sky-700 dark:text-sky-300"
                  : "bg-primary/10 text-primary"
              )}
            >
              {outbound ? t("outbound") : t("inbound")}
            </span>
            <span className="truncate text-sm font-medium">
              {outbound
                ? t("transcriptTo", { name: peerTitle })
                : t("transcriptFrom", { name: peerTitle })}
            </span>
          </div>
          <div className="mt-1 truncate text-xs text-muted-foreground">
            Session {peer.conversationId}
            {peer.agentType ? ` · ${peer.agentType}` : ""}
            {sentAt ? ` · ${sentAt}` : ""}
          </div>
        </div>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="h-7 w-7 shrink-0"
          title={t("openSession")}
          aria-label={t("openSession")}
          disabled={!peerConversation}
          onClick={openPeer}
        >
          <ArrowUpRight className="h-3.5 w-3.5" />
        </Button>
      </header>
      <div className="mt-2.5 text-sm" data-collaboration-body>
        <ContentPartsRenderer
          parts={bodyParts}
          role={outbound ? "user" : "assistant"}
        />
      </div>
    </article>
  )
}
