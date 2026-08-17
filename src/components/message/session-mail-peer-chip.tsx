"use client"

import { useTranslations } from "next-intl"

import { ReferenceBadge } from "@/components/chat/composer/badges/reference-badge"
import { useTabActions } from "@/contexts/tab-context"
import {
  formatConversationTitle,
  formatSessionMailName,
} from "@/lib/conversation-title"
import type { AgentType } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"

export function SessionMailPeerChip({
  conversationId,
  title,
  agentType,
  prefix,
}: {
  conversationId: number
  title?: string | null
  agentType?: string | null
  prefix: string
}) {
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const peerConversation = conversations.find(
    (conversation) => conversation.id === conversationId
  )
  const { openTab } = useTabActions()
  const fullTitle =
    formatConversationTitle(peerConversation?.title ?? title) ||
    `Session ${conversationId}`
  const name = formatSessionMailName(
    peerConversation?.title ?? title,
    conversationId
  )
  const hint = `${fullTitle} · #${conversationId}`

  return (
    <span className="inline-flex min-w-0 max-w-full items-center gap-1">
      <span className="shrink-0">{prefix}</span>
      <button
        type="button"
        className="inline-flex min-w-0 max-w-full items-center gap-1 appearance-none text-left"
        title={hint}
        aria-label={`${prefix} ${hint}`}
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
        <ReferenceBadge
          data={{
            refType: "session",
            id: String(conversationId),
            label: name,
            uri: `codeg://session/${conversationId}`,
            meta: {
              agentType: (peerConversation?.agent_type ??
                agentType) as AgentType | undefined,
            },
          }}
        />
        <span
          className="shrink-0 tabular-nums text-[0.7em] text-muted-foreground/65"
          data-session-mail-id={conversationId}
        >
          #{conversationId}
        </span>
      </button>
    </span>
  )
}

export function SessionMailFromBadge({
  conversationId,
  title,
  agentType,
}: {
  conversationId: number
  title?: string | null
  agentType?: string | null
}) {
  const t = useTranslations("Collaboration")
  return (
    <div className="mb-1 flex w-fit max-w-full items-center self-end text-[0.6875rem] text-muted-foreground">
      <SessionMailPeerChip
        conversationId={conversationId}
        title={title}
        agentType={agentType}
        prefix={t("fromPrefix")}
      />
    </div>
  )
}
