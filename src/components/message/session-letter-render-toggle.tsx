"use client"

import type { ReactNode } from "react"
import { Braces, CheckIcon, CopyIcon, Mails } from "lucide-react"
import { useTranslations } from "next-intl"

import { MessageAction } from "@/components/ai-elements/message"
import { useCopiedFlag } from "@/hooks/use-copied-flag"
import { cn, copyTextToClipboard } from "@/lib/utils"
import { useSessionLetterUiStore } from "@/stores/session-letter-ui-store"

export function SessionLetterCopyButton({
  text,
  className,
}: {
  text: string
  className?: string
}) {
  const t = useTranslations("Folder.chat.messageList")
  const [copied, markCopied] = useCopiedFlag(2000)

  return (
    <MessageAction
      tooltip={copied ? t("copied") : t("copyMessage")}
      className={className}
      size="icon-xs"
      data-session-letter-copy=""
      disabled={!text}
      onClick={async () => {
        if (!text || copied) return
        if (await copyTextToClipboard(text)) markCopied()
      }}
    >
      {copied ? <CheckIcon size={12} /> : <CopyIcon size={12} />}
    </MessageAction>
  )
}

export function SessionLetterPreviewToggle({
  letterKey,
  className,
}: {
  letterKey: string
  className?: string
}) {
  const t = useTranslations("Collaboration")
  const raw = useSessionLetterUiStore((state) => state.isMcpPreview(letterKey))
  const togglePreview = useSessionLetterUiStore((state) => state.togglePreview)
  const label = raw ? t("letterRenderToCustom") : t("letterRenderToMcp")

  return (
    <MessageAction
      tooltip={label}
      className={className}
      size="icon-xs"
      aria-pressed={raw}
      data-session-letter-preview={raw ? "mcp" : "custom"}
      onClick={() => togglePreview(letterKey)}
    >
      {raw ? <Braces size={12} /> : <Mails size={12} />}
    </MessageAction>
  )
}

export function SessionLetterActions({
  letterKey,
  copyText,
  className,
}: {
  letterKey: string
  copyText?: string | null
  className?: string
}) {
  return (
    <div
      className={cn(
        "mb-0.5 flex shrink-0 items-center gap-1 self-end opacity-0 transition-opacity group-hover/letter:opacity-100 group-focus-within/letter:opacity-100",
        className
      )}
    >
      {copyText ? <SessionLetterCopyButton text={copyText} /> : null}
      <SessionLetterPreviewToggle letterKey={letterKey} />
    </div>
  )
}

export function SessionLetterFrame({
  letterKey,
  eventId,
  copyText,
  children,
}: {
  letterKey: string
  eventId?: string | null
  copyText?: string | null
  children: ReactNode
}) {
  const focusedEventId = useSessionLetterUiStore(
    (state) => state.focusedEventId
  )
  const focused = eventId != null && focusedEventId === eventId

  return (
    <div
      className={cn(
        "group/letter flex w-fit max-w-full items-end gap-1",
        focused && "rounded-lg ring-2 ring-primary/35"
      )}
      data-session-letter-frame=""
      data-letter-event-id={eventId ?? undefined}
    >
      <div className="min-w-0">{children}</div>
      <SessionLetterActions letterKey={letterKey} copyText={copyText} />
    </div>
  )
}
