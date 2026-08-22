import { parseTimestamp } from "@/components/conversations/sidebar-conversation-grouping"
import type {
  WorkTask,
  WorkTaskBusinessStatus,
  WorkTaskStatus,
} from "@/lib/types"

/** Four stable board columns over the user-facing business workflow. */
export type BoardColumnId = "todo" | "inProgress" | "attention" | "done"

export const BOARD_COLUMN_IDS: BoardColumnId[] = [
  "todo",
  "inProgress",
  "attention",
  "done",
]

/**
 * The exact business statuses behind each column, written out as a table.
 * `columnForStatus` stays the single source of truth for the mapping; this is
 * its spec copy, and board-columns.test.ts asserts the two agree and that every
 * `WorkTaskStatus` appears here exactly once — which is what makes adding a
 * status without filing it in a column a test failure rather than a silent
 * disappearance from the board.
 */
export const STATUSES_BY_COLUMN: Record<
  BoardColumnId,
  WorkTaskBusinessStatus[]
> = {
  todo: ["todo"],
  inProgress: ["in_progress"],
  attention: ["blocked", "review"],
  done: ["done", "canceled"],
}

/** Every status, column order — the flattened spec table (see above). */
export const ALL_WORK_TASK_BUSINESS_STATUSES: WorkTaskBusinessStatus[] =
  BOARD_COLUMN_IDS.flatMap((col) => STATUSES_BY_COLUMN[col])

/**
 * Business status → board column. `canceled` lives in Done but is hidden
 * unless the "show canceled" toggle is on (filtered by `groupTasksByColumn`).
 */
export function columnForStatus(
  status: WorkTaskBusinessStatus
): BoardColumnId {
  switch (status) {
    case "todo":
      return "todo"
    case "in_progress":
      return "inProgress"
    case "blocked":
    case "review":
      return "attention"
    case "done":
    case "canceled":
      return "done"
  }
}

/**
 * Bucket tasks into the four columns. Canceled tasks are dropped unless
 * `showCanceled`, archived ones unless `showArchived` (archived is always
 * terminal).
 *
 * Todo / in-progress / done read freshest-first: `updated_at` descending, so
 * whatever just moved sits at the top of its column. Attention is the
 * exception — it mixes four meanings, so it sorts by severity first
 * (`failed` > `awaiting_input` > `review` > `merging`) and only then by
 * freshest, so a dead card is never buried under a merge in progress.
 *
 * The sort is stable and the backend hands rows over in board order
 * (sort_order, id), so equal timestamps keep that order — which is exactly
 * what preserves a pending-column drag: `reorder` stamps the whole column
 * with one `updated_at`, the rows tie, and the fallback is their freshly
 * written sort_order. sort_order still drives the launch queue
 * (`next_queued` / "start all"); it just no longer drives the display.
 */
export function groupTasksByColumn(
  tasks: WorkTask[],
  showCanceled: boolean,
  showArchived = false
): Record<BoardColumnId, WorkTask[]> {
  const grouped: Record<BoardColumnId, WorkTask[]> = {
    todo: [],
    inProgress: [],
    attention: [],
    done: [],
  }
  for (const task of tasks) {
    if (task.task_status === "canceled" && !showCanceled) continue
    if (task.archived_at != null && !showArchived) continue
    grouped[columnForStatus(task.task_status)].push(task)
  }
  for (const column of BOARD_COLUMN_IDS) {
    grouped[column].sort(
      column === "attention" ? byAttentionSeverityThenFreshest : byFreshest
    )
  }
  return grouped
}

/** Freshest-first: the order todo / in-progress / done and the list view
 *  read in. Attention uses `byAttentionSeverityThenFreshest` instead. */
function byFreshest(a: WorkTask, b: WorkTask): number {
  return parseTimestamp(b.updated_at) - parseTimestamp(a.updated_at)
}

/** Lower is more urgent. Statuses that cannot land in attention are last. */
function attentionSeverity(
  status: WorkTaskBusinessStatus,
  engineStatus: WorkTaskStatus
): number {
  if (status === "blocked") return engineStatus === "failed" ? 0 : 1
  if (status === "review") return engineStatus === "merging" ? 3 : 2
  return 4
}

function byAttentionSeverityThenFreshest(a: WorkTask, b: WorkTask): number {
  const bySeverity =
    attentionSeverity(a.task_status, a.status) -
    attentionSeverity(b.task_status, b.status)
  if (bySeverity !== 0) return bySeverity
  return byFreshest(a, b)
}

/**
 * The list view's rows: one flat, freshest-first sequence instead of four
 * columns. `group` is the list-only status filter — a whole board COLUMN rather
 * than individual statuses (`null` = every status, which is the default), so
 * the list narrows by the same four buckets the board sorts into and the two
 * views can't describe the same tasks with different words.
 *
 * The visibility toggles apply exactly as they do on the board: `showCanceled`
 * and `showArchived` are one pair of controls shared by both views. (They have
 * to be: a column-wide filter cannot single out `canceled`, which lives inside
 * the Done column.)
 */
export function filterTasksForList(
  tasks: WorkTask[],
  group: BoardColumnId | null,
  showCanceled: boolean,
  showArchived: boolean
): WorkTask[] {
  return tasks
    .filter(
      (task) =>
        (task.task_status !== "canceled" || showCanceled) &&
        (task.archived_at == null || showArchived) &&
        (group == null || columnForStatus(task.task_status) === group)
    )
    .sort(byFreshest)
}
