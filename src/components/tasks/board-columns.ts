import { parseTimestamp } from "@/components/conversations/sidebar-conversation-grouping"
import type { WorkTask, WorkTaskBusinessStatus } from "@/lib/types"

/** Fixed product workflow. Every business status owns exactly one column. */
export type BoardColumnId =
  | "backlog"
  | "todo"
  | "inProgress"
  | "review"
  | "done"
  | "blocked"
  | "canceled"

export const BOARD_COLUMN_IDS: BoardColumnId[] = [
  "backlog",
  "todo",
  "inProgress",
  "review",
  "done",
  "blocked",
  "canceled",
]

export const STATUSES_BY_COLUMN: Record<
  BoardColumnId,
  WorkTaskBusinessStatus[]
> = {
  backlog: ["backlog"],
  todo: ["todo"],
  inProgress: ["in_progress"],
  review: ["review"],
  done: ["done"],
  blocked: ["blocked"],
  canceled: ["canceled"],
}

export const ALL_WORK_TASK_BUSINESS_STATUSES: WorkTaskBusinessStatus[] =
  BOARD_COLUMN_IDS.flatMap((column) => STATUSES_BY_COLUMN[column])

export function columnForStatus(status: WorkTaskBusinessStatus): BoardColumnId {
  return status === "in_progress" ? "inProgress" : status
}

export function statusForColumn(column: BoardColumnId): WorkTaskBusinessStatus {
  return column === "inProgress" ? "in_progress" : column
}

export function groupTasksByColumn(
  tasks: WorkTask[],
  showArchived = false
): Record<BoardColumnId, WorkTask[]> {
  const grouped: Record<BoardColumnId, WorkTask[]> = {
    backlog: [],
    todo: [],
    inProgress: [],
    review: [],
    done: [],
    blocked: [],
    canceled: [],
  }
  for (const task of tasks) {
    if (task.archived_at != null && !showArchived) continue
    grouped[columnForStatus(task.task_status)].push(task)
  }
  for (const column of BOARD_COLUMN_IDS) grouped[column].sort(byFreshest)
  return grouped
}

function byFreshest(a: WorkTask, b: WorkTask): number {
  return parseTimestamp(b.updated_at) - parseTimestamp(a.updated_at)
}

export function filterTasksForList(
  tasks: WorkTask[],
  group: BoardColumnId | null,
  showArchived: boolean
): WorkTask[] {
  return tasks
    .filter(
      (task) =>
        (task.archived_at == null || showArchived) &&
        (group == null || columnForStatus(task.task_status) === group)
    )
    .sort(byFreshest)
}
