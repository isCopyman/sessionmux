"use client"

const BOARD_FILTER_KEY = "workspace:tasks-board-filter"
const VIEW_MODE_KEY = "workspace:tasks-view-mode"
const STATUS_FILTER_KEY = "workspace:tasks-status-filter"
const GROUPING_KEY = "workspace:tasks-board-grouping"

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
export const TASKS_BOARD_GROUPINGS = ["none", "folder", "agent"] as const
export type TasksBoardGrouping = (typeof TASKS_BOARD_GROUPINGS)[number]
export const DEFAULT_TASKS_BOARD_GROUPING: TasksBoardGrouping = "none"

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

export function loadTasksStatusFilter(): TasksStatusGroup | null {
  if (typeof window === "undefined") return null
  try {
    const raw = localStorage.getItem(STATUS_FILTER_KEY)
    return TASKS_STATUS_GROUPS.find((group) => group === raw) ?? null
  } catch {
    return null
  }
}

export function saveTasksStatusFilter(group: TasksStatusGroup | null): void {
  if (typeof window === "undefined") return
  try {
    if (group == null) localStorage.removeItem(STATUS_FILTER_KEY)
    else localStorage.setItem(STATUS_FILTER_KEY, group)
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
