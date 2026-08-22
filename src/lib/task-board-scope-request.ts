"use client"

import {
  saveTasksScope,
  type TasksScope,
} from "@/lib/tasks-board-filter-storage"

export const TASK_BOARD_SCOPE_REQUEST_EVENT = "task-board://scope-requested"

let pendingTaskScope: TasksScope | null = null

export function requestTaskBoardScope(scope: TasksScope): void {
  pendingTaskScope = scope
  saveTasksScope(scope)
  if (typeof window !== "undefined") {
    window.dispatchEvent(
      new CustomEvent<TasksScope>(TASK_BOARD_SCOPE_REQUEST_EVENT, {
        detail: scope,
      })
    )
  }
}

export function consumePendingTaskBoardScope(): TasksScope | null {
  const scope = pendingTaskScope
  pendingTaskScope = null
  return scope
}
