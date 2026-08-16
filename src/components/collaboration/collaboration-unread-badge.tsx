"use client"

import { useTranslations } from "next-intl"
import { cn } from "@/lib/utils"

export function CollaborationUnreadBadge({
  count,
  className,
}: {
  count: number
  className?: string
}) {
  const t = useTranslations("Collaboration")
  if (count <= 0) return null
  const label = t("unreadCount", { count })
  return (
    <span
      className={cn(
        "inline-flex h-[1.125rem] min-w-[1.125rem] shrink-0 items-center justify-center rounded-full bg-primary px-1 text-[0.625rem] font-semibold leading-none tabular-nums text-primary-foreground",
        className
      )}
      title={label}
      aria-label={label}
    >
      {count > 99 ? "99+" : count}
    </span>
  )
}
