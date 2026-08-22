"use client"

import { useTranslations } from "next-intl"
import { cn } from "@/lib/utils"
import type { WorkTaskPriority } from "@/lib/types"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

export const TASK_PRIORITIES: WorkTaskPriority[] = [
  "none",
  "low",
  "medium",
  "high",
  "urgent",
]

export const PRIORITY_LABEL_KEYS = {
  none: "priorityNone",
  low: "priorityLow",
  medium: "priorityMedium",
  high: "priorityHigh",
  urgent: "priorityUrgent",
} as const satisfies Record<WorkTaskPriority, string>

export const taskPriorityRank = (priority?: WorkTaskPriority): number =>
  TASK_PRIORITIES.indexOf(priority ?? "none")

const COLOR: Record<WorkTaskPriority, string> = {
  none: "text-muted-foreground/45",
  low: "text-sky-500",
  medium: "text-amber-500",
  high: "text-orange-500",
  urgent: "text-rose-500",
}

/** The ascending bars used by Multica: compact enough for cards and menus. */
export function TaskPriorityIcon({
  priority,
  className,
  showNone = false,
}: {
  priority?: WorkTaskPriority
  className?: string
  showNone?: boolean
}) {
  const value = priority ?? "none"
  if (value === "none" && !showNone) return null
  const active = taskPriorityRank(value)
  return (
    <svg
      viewBox="0 0 16 16"
      data-task-priority-icon={value}
      className={cn("size-4 shrink-0", COLOR[value], className)}
      fill="currentColor"
      aria-hidden="true"
    >
      {[4, 7, 10, 13].map((height, index) => (
        <rect
          key={height}
          x={1 + index * 4}
          y={15 - height}
          width="2.5"
          height={height}
          rx="0.75"
          opacity={value === "none" ? 0.32 : index < active ? 1 : 0.18}
        />
      ))}
    </svg>
  )
}

export function TaskPrioritySelect({
  value,
  onValueChange,
  className,
  ariaLabel,
}: {
  value: WorkTaskPriority
  onValueChange: (value: WorkTaskPriority) => void
  className?: string
  ariaLabel?: string
}) {
  const t = useTranslations("Tasks")
  return (
    <Select
      value={value}
      onValueChange={(next) => onValueChange(next as WorkTaskPriority)}
    >
      <SelectTrigger
        className={className}
        aria-label={ariaLabel ?? t("priority")}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {TASK_PRIORITIES.map((priority) => (
          <SelectItem key={priority} value={priority}>
            <span className="flex items-center gap-2">
              <TaskPriorityIcon priority={priority} showNone />
              {t(PRIORITY_LABEL_KEYS[priority])}
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
