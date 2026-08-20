"use client"

import { cn } from "@/lib/utils"

export function TranscriptDateSeparator({
  label,
  className,
}: {
  label: string
  className?: string
}) {
  return (
    <div
      role="separator"
      data-transcript-date-separator=""
      className={cn("flex items-center gap-2 px-1 py-2", className)}
    >
      <span aria-hidden="true" className="h-px flex-1 bg-border" />
      <span className="shrink-0 rounded-full border border-border bg-muted/50 px-2 py-0.5 text-[0.625rem] font-medium leading-none text-muted-foreground">
        {label}
      </span>
      <span aria-hidden="true" className="h-px flex-1 bg-border" />
    </div>
  )
}
