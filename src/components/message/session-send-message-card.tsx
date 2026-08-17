"use client"

import { useMemo } from "react"
import { useTranslations } from "next-intl"

import { parseSessionSendMessageInput } from "@/lib/session-send-message-tool"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { ContentPartsRenderer } from "./content-parts-renderer"
import { SessionMailPeerChip } from "./session-mail-peer-chip"

export function SessionSendMessageCard({
  input,
}: {
  input: string | null
}) {
  const t = useTranslations("Collaboration")
  const parsed = parseSessionSendMessageInput(input)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const targetId = parsed?.targetSessionIds[0]
  const target = conversations.find(
    (conversation) => conversation.id === targetId
  )
  const bodyParts = useMemo(
    () => [{ type: "text" as const, text: parsed?.content ?? "" }],
    [parsed?.content]
  )

  if (!parsed || targetId == null) return null

  const extra =
    parsed.targetSessionIds.length > 1
      ? ` +${parsed.targetSessionIds.length - 1}`
      : ""

  return (
    <article
      className="w-full max-w-[min(40rem,88%)] rounded-lg border border-border bg-muted/30 px-3 py-2.5"
      data-session-send-message=""
      data-target-session-id={targetId}
    >
      <header className="mb-1.5 flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
        <SessionMailPeerChip
          conversationId={targetId}
          title={target?.title}
          agentType={target?.agent_type}
          prefix={t("toPrefix")}
        />
        {extra ? <span className="shrink-0">{extra}</span> : null}
      </header>
      <div className="text-sm">
        <ContentPartsRenderer parts={bodyParts} role="assistant" />
      </div>
    </article>
  )
}
