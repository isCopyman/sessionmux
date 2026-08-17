"use client"

import { Braces, Mails } from "lucide-react"
import { useTranslations } from "next-intl"

import { cn } from "@/lib/utils"
import { useSessionLetterRenderStore } from "@/stores/session-letter-render-store"

export function SessionLetterRenderToggle({
  className,
  compact = false,
}: {
  className?: string
  compact?: boolean
}) {
  const t = useTranslations("Collaboration")
  const mode = useSessionLetterRenderStore((state) => state.mode)
  const setMode = useSessionLetterRenderStore((state) => state.setMode)
  const raw = mode === "mcp"
  const label = raw ? t("letterRenderToCustom") : t("letterRenderToMcp")

  return (
    <button
      type="button"
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
        compact ? "h-6 w-6" : "h-7 w-7",
        raw && "bg-muted text-foreground",
        className
      )}
      aria-pressed={raw}
      aria-label={label}
      title={label}
      data-session-letter-render={mode}
      onClick={() => setMode(raw ? "custom" : "mcp")}
    >
      {raw ? (
        <Braces className={compact ? "h-3.5 w-3.5" : "h-4 w-4"} />
      ) : (
        <Mails className={compact ? "h-3.5 w-3.5" : "h-4 w-4"} />
      )}
    </button>
  )
}
