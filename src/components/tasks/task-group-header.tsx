import { cn } from "@/lib/utils"

/**
 * Column-internal (and list) grouping header: a quiet label plus the
 * segment's count. Hidden entirely when grouping is off, and when project
 * grouping has already been narrowed by the folder filter.
 */
export function TaskGroupHeader({
  label,
  count,
  className,
}: {
  label: string
  count: number
  className?: string
}) {
  return (
    <div
      data-testid="task-group-header"
      className={cn("flex items-center gap-1.5 px-0.5", className)}
    >
      <span className="min-w-0 truncate text-[0.625rem] font-medium text-muted-foreground">
        {label}
      </span>
      <span className="rounded-full bg-muted/70 px-1.5 py-0.5 text-[0.625rem] font-medium leading-none text-muted-foreground tabular-nums">
        {count}
      </span>
    </div>
  )
}
