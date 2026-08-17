"use client"

import { useMemo } from "react"
import { ArrowUpRight } from "lucide-react"
import { useTranslations } from "next-intl"

import { Button } from "@/components/ui/button"
import { useTabActions } from "@/contexts/tab-context"
import {
  formatConversationTitle,
  formatSessionMailName,
} from "@/lib/conversation-title"
import { parseSessionSendMessageInput } from "@/lib/session-send-message-tool"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { ContentPartsRenderer } from "./content-parts-renderer"

export function SessionSendMessageCard({
  input,
}: {
  input: string | null
}) {
  const t = useTranslations("Collaboration")
  const parsed = parseSessionSendMessageInput(input)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const { openTab } = useTabActions()
  const targetId = parsed?.targetSessionIds[0]
  const target = conversations.find(
    (conversation) => conversation.id === targetId
  )
  const bodyParts = useMemo(
    () => [{ type: "text" as const, text: parsed?.content ?? "" }],
    [parsed?.content]
  )

  if (!parsed || targetId == null) return null

  const name = formatSessionMailName(target?.title, targetId)
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
        <span
          className="truncate font-medium text-foreground"
          title={target?.title ?? name}
        >
          {t("toSession", { name })}
          {extra}
        </span>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="ml-auto h-6 w-6 shrink-0"
          title={t("openSession")}
          aria-label={t("openSession")}
          disabled={!target}
          onClick={() => {
            if (!target) return
            openTab(
              target.folder_id,
              target.id,
              target.agent_type,
              true,
              formatConversationTitle(target.title) || undefined
            )
          }}
        >
          <ArrowUpRight className="h-3.5 w-3.5" />
        </Button>
      </header>
      <div className="text-sm">
        <ContentPartsRenderer parts={bodyParts} role="assistant" />
      </div>
    </article>
  )
}
