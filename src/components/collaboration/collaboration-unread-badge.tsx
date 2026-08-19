"use client"

import { cn } from "@/lib/utils"

/**
 * Small muted unread count for a Room row in a sidebar tree. Renders
 * nothing at count <= 0. This is the plain inline style the tree views
 * already use; it is not the pill/avatar-overlay style, which has no
 * current caller.
 */
export function CollaborationUnreadBadge({
  count,
  className,
}: {
  count: number
  className?: string
}) {
  if (count <= 0) return null
  return (
    <span
      className={cn("shrink-0 text-[10px] text-muted-foreground", className)}
    >
      {count}
    </span>
  )
}
