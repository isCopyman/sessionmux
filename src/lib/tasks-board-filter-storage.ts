"use client"

import type { WorkTaskPriority } from "@/lib/types"

const BOARD_FILTER_KEY = "workspace:tasks-board-filter"
const VIEW_MODE_KEY = "workspace:tasks-view-mode"
const STATUS_FILTER_KEY = "workspace:tasks-status-filter"
const GROUPING_KEY = "workspace:tasks-board-grouping"
const SCOPE_FILTER_KEY = "workspace:tasks-scope-filter"
const OWNER_FILTER_KEY = "workspace:tasks-owner-filter"
const PRIORITY_FILTER_KEY = "workspace:tasks-priority-filter"
const SORT_KEY = "workspace:tasks-sort"

/**
 * Filters describe what one concrete Board Tab is looking at, so Workbenches
 * must not overwrite each other. Keep the historical key for the legacy
 * standalone global Board; scope every Workbench Tab key.
 * Presentation preferences such as grouping, sorting and hidden columns stay
 * global on purpose.
 */
function scopedFilterKey(base: string, boardViewKey: string): string {
  return boardViewKey === "global" ? base : `${base}:${boardViewKey}`
}

export const TASKS_STATUS_GROUPS = [
  "backlog",
  "todo",
  "inProgress",
  "review",
  "done",
  "blocked",
  "canceled",
] as const
export type TasksStatusGroup = (typeof TASKS_STATUS_GROUPS)[number]

export interface TasksBoardFilter {
  showArchived: boolean
  hiddenColumns: TasksStatusGroup[]
}
export const DEFAULT_TASKS_BOARD_FILTER: TasksBoardFilter = {
  showArchived: false,
  hiddenColumns: [],
}

export type TasksViewMode = "board" | "list"
export const DEFAULT_TASKS_VIEW_MODE: TasksViewMode = "board"
export const TASKS_BOARD_GROUPINGS = [
  "none",
  "folder",
  "session",
  "agent",
] as const
export type TasksBoardGrouping = (typeof TASKS_BOARD_GROUPINGS)[number]
export const DEFAULT_TASKS_BOARD_GROUPING: TasksBoardGrouping = "none"
export const TASKS_SCOPES = ["all", "agent", "attention"] as const
export type TasksScope = (typeof TASKS_SCOPES)[number]
export const DEFAULT_TASKS_SCOPE: TasksScope = "all"
export const TASKS_SORTS = ["manual", "priority", "updated"] as const
export type TasksSort = (typeof TASKS_SORTS)[number]
export const DEFAULT_TASKS_SORT: TasksSort = "manual"

export function loadTasksBoardFilter(): TasksBoardFilter {
  if (typeof window === "undefined") return DEFAULT_TASKS_BOARD_FILTER
  try {
    const raw = localStorage.getItem(BOARD_FILTER_KEY)
    if (!raw) return DEFAULT_TASKS_BOARD_FILTER
    const value = JSON.parse(raw) as Record<string, unknown>
    const hiddenColumns = Array.isArray(value.hiddenColumns)
      ? value.hiddenColumns.filter((column): column is TasksStatusGroup =>
          TASKS_STATUS_GROUPS.includes(column as TasksStatusGroup)
        )
      : value.showCanceled === false
        ? ["canceled" as const]
        : []
    return {
      hiddenColumns: [...new Set(hiddenColumns)],
      showArchived:
        typeof value.showArchived === "boolean" ? value.showArchived : false,
    }
  } catch {
    return DEFAULT_TASKS_BOARD_FILTER
  }
}

export function saveTasksBoardFilter(filter: TasksBoardFilter): void {
  if (typeof window === "undefined") return
  try {
    localStorage.setItem(BOARD_FILTER_KEY, JSON.stringify(filter))
  } catch {}
}

export function loadTasksViewMode(): TasksViewMode {
  if (typeof window === "undefined") return DEFAULT_TASKS_VIEW_MODE
  try {
    const raw = localStorage.getItem(VIEW_MODE_KEY)
    return raw === "list" || raw === "board" ? raw : DEFAULT_TASKS_VIEW_MODE
  } catch {
    return DEFAULT_TASKS_VIEW_MODE
  }
}

export function saveTasksViewMode(mode: TasksViewMode): void {
  if (typeof window === "undefined") return
  try {
    localStorage.setItem(VIEW_MODE_KEY, mode)
  } catch {}
}

export function loadTasksStatusFilter(
  boardViewKey = "global"
): TasksStatusGroup | null {
  if (typeof window === "undefined") return null
  try {
    const raw = localStorage.getItem(
      scopedFilterKey(STATUS_FILTER_KEY, boardViewKey)
    )
    return TASKS_STATUS_GROUPS.find((group) => group === raw) ?? null
  } catch {
    return null
  }
}

export function saveTasksStatusFilter(
  group: TasksStatusGroup | null,
  boardViewKey = "global"
): void {
  if (typeof window === "undefined") return
  try {
    const key = scopedFilterKey(STATUS_FILTER_KEY, boardViewKey)
    if (group == null) localStorage.removeItem(key)
    else localStorage.setItem(key, group)
  } catch {}
}

export function loadTasksBoardGrouping(): TasksBoardGrouping {
  if (typeof window === "undefined") return DEFAULT_TASKS_BOARD_GROUPING
  try {
    const raw = localStorage.getItem(GROUPING_KEY)
    return TASKS_BOARD_GROUPINGS.find((group) => group === raw) ?? "none"
  } catch {
    return "none"
  }
}

export function saveTasksBoardGrouping(grouping: TasksBoardGrouping): void {
  if (typeof window === "undefined") return
  try {
    localStorage.setItem(GROUPING_KEY, grouping)
  } catch {}
}

export function loadTasksScope(boardViewKey = "global"): TasksScope {
  if (typeof window === "undefined") return DEFAULT_TASKS_SCOPE
  try {
    const raw = localStorage.getItem(
      scopedFilterKey(SCOPE_FILTER_KEY, boardViewKey)
    )
    // The first quick-view experiment exposed unassigned/manual as two tabs.
    // Both are execution-mode filters rather than durable top-level views, so
    // old preferences migrate to the neutral overview instead of stranding a
    // user on a tab that no longer exists.
    if (raw === "unassigned" || raw === "manual") return DEFAULT_TASKS_SCOPE
    return TASKS_SCOPES.find((scope) => scope === raw) ?? DEFAULT_TASKS_SCOPE
  } catch {
    return DEFAULT_TASKS_SCOPE
  }
}

export function saveTasksScope(
  scope: TasksScope,
  boardViewKey = "global"
): void {
  if (typeof window === "undefined") return
  try {
    localStorage.setItem(scopedFilterKey(SCOPE_FILTER_KEY, boardViewKey), scope)
  } catch {}
}

export function loadTasksOwnerFilter(boardViewKey = "global"): number | null {
  if (typeof window === "undefined") return null
  try {
    const raw = localStorage.getItem(
      scopedFilterKey(OWNER_FILTER_KEY, boardViewKey)
    )
    if (raw == null) return null
    const value = Number(raw)
    return Number.isSafeInteger(value) && value > 0 ? value : null
  } catch {
    return null
  }
}

export function saveTasksOwnerFilter(
  conversationId: number | null,
  boardViewKey = "global"
): void {
  if (typeof window === "undefined") return
  try {
    const key = scopedFilterKey(OWNER_FILTER_KEY, boardViewKey)
    if (conversationId == null) localStorage.removeItem(key)
    else localStorage.setItem(key, String(conversationId))
  } catch {}
}

const TASK_PRIORITIES: WorkTaskPriority[] = [
  "none",
  "low",
  "medium",
  "high",
  "urgent",
]

export function loadTasksPriorityFilter(
  boardViewKey = "global"
): WorkTaskPriority | null {
  if (typeof window === "undefined") return null
  try {
    const raw = localStorage.getItem(
      scopedFilterKey(PRIORITY_FILTER_KEY, boardViewKey)
    )
    return TASK_PRIORITIES.find((priority) => priority === raw) ?? null
  } catch {
    return null
  }
}

export function saveTasksPriorityFilter(
  priority: WorkTaskPriority | null,
  boardViewKey = "global"
): void {
  if (typeof window === "undefined") return
  try {
    const key = scopedFilterKey(PRIORITY_FILTER_KEY, boardViewKey)
    if (priority == null) localStorage.removeItem(key)
    else localStorage.setItem(key, priority)
  } catch {}
}

export function loadTasksSort(): TasksSort {
  if (typeof window === "undefined") return DEFAULT_TASKS_SORT
  try {
    const raw = localStorage.getItem(SORT_KEY)
    return TASKS_SORTS.find((sort) => sort === raw) ?? DEFAULT_TASKS_SORT
  } catch {
    return DEFAULT_TASKS_SORT
  }
}

export function saveTasksSort(sort: TasksSort): void {
  if (typeof window === "undefined") return
  try {
    localStorage.setItem(SORT_KEY, sort)
  } catch {}
}
