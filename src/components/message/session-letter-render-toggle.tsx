"use client"

import type { ReactNode } from "react"
import { Braces, Mails } from "lucide-react"
import { useTranslations } from "next-intl"

import { cn } from "@/lib/utils"
import { useSessionLetterUiStore } from "@/stores/session-letter-ui-store"

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
    <button
      type="button"
      className={cn(
        "inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
        raw && "text-foreground",
        className
      )}
      aria-pressed={raw}
      aria-label={label}
      title={label}
      data-session-letter-preview={raw ? "mcp" : "custom"}
      onClick={() => togglePreview(letterKey)}
    >
      {raw ? <Braces className="h-3.5 w-3.5" /> : <Mails className="h-3.5 w-3.5" />}
    </button>
  )
}

export function SessionLetterFrame({
  letterKey,
  eventId,
  children,
}: {
  letterKey: string
  eventId?: string | null
  children: ReactNode
}) {
  const focusedEventId = useSessionLetterUiStore((state) => state.focusedEventId)
  const focused = eventId != null && focusedEventId === eventId

  return (
    <div
      className={cn(
        "group/letter flex w-fit max-w-full items-end gap-0.5",
        focused && "rounded-lg ring-2 ring-primary/35"
      )}
      data-session-letter-frame=""
      data-letter-event-id={eventId ?? undefined}
    >
      <div className="min-w-0">{children}</div>
      <SessionLetterPreviewToggle
        letterKey={letterKey}
        className="mb-0.5 opacity-0 transition-opacity group-hover/letter:opacity-100 group-focus-within/letter:opacity-100"
      />
    </div>
  )
}
