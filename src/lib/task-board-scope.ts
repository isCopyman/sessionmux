export type TaskBoardScope = "global" | `project:${number}`

export const GLOBAL_TASK_BOARD_SCOPE: TaskBoardScope = "global"

export function projectTaskBoardScope(folderId: number): TaskBoardScope {
  return `project:${folderId}`
}

export function taskBoardProjectId(scope: TaskBoardScope): number | null {
  if (scope === GLOBAL_TASK_BOARD_SCOPE) return null
  const match = /^project:(\d+)$/.exec(scope)
  if (!match) return null
  const id = Number(match[1])
  return Number.isSafeInteger(id) && id > 0 ? id : null
}

export function isTaskBoardScope(value: unknown): value is TaskBoardScope {
  return (
    value === GLOBAL_TASK_BOARD_SCOPE ||
    (typeof value === "string" &&
      taskBoardProjectId(value as TaskBoardScope) != null)
  )
}
