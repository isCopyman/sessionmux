import { parseTimestamp } from "@/components/conversations/sidebar-conversation-grouping"
import type { WorkTask, WorkTaskStatus } from "@/lib/types"

/** The four board columns (DB statuses are exact; the UI aggregates them). */
export type BoardColumnId = "todo" | "inProgress" | "attention" | "done"

export const BOARD_COLUMN_IDS: BoardColumnId[] = [
  "todo",
  "inProgress",
  "attention",
  "done",
]

/**
 * The exact statuses behind each column, written out as a table.
 * `columnForStatus` stays the single source of truth for the mapping; this is
 * its spec copy, and board-columns.test.ts asserts the two agree and that every
 * `WorkTaskStatus` appears here exactly once — which is what makes adding a
 * status without filing it in a column a test failure rather than a silent
 * disappearance from the board.
 */
export const STATUSES_BY_COLUMN: Record<BoardColumnId, WorkTaskStatus[]> = {
  todo: ["todo", "queued"],
  inProgress: ["preparing", "running"],
  attention: ["awaiting_input", "review", "merging", "failed"],
  done: ["done", "canceled"],
}

/** Every status, column order — the flattened spec table (see above). */
export const ALL_WORK_TASK_STATUSES: WorkTaskStatus[] =
  BOARD_COLUMN_IDS.flatMap((col) => STATUSES_BY_COLUMN[col])

/**
 * DB status → board column. `canceled` lives in the Done column but is hidden
 * unless the "show canceled" toggle is on (filtered by `groupTasksByColumn`).
 */
export function columnForStatus(status: WorkTaskStatus): BoardColumnId {
  switch (status) {
    case "todo":
    // Still waiting for a concurrency slot — nothing is happening yet.
    case "queued":
      return "todo"
    // Already out of the queue and working (worktree, init command, agent
    // spawn), just without a session to show yet.
    case "preparing":
    case "running":
      return "inProgress"
    case "awaiting_input":
    case "review":
    // A merge is an agent turn, but the card must not bounce across the
    // board when the user clicks merge — it stays in the review column and
    // moves straight to Done when the merge lands.
    case "merging":
    case "failed":
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
    if (task.status === "canceled" && !showCanceled) continue
    if (task.archived_at != null && !showArchived) continue
    grouped[columnForStatus(task.status)].push(task)
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
function attentionSeverity(status: WorkTaskStatus): number {
  switch (status) {
    case "failed":
      return 0
    case "awaiting_input":
      return 1
    case "review":
      return 2
    case "merging":
      return 3
    default:
      return 4
  }
}

function byAttentionSeverityThenFreshest(a: WorkTask, b: WorkTask): number {
  const bySeverity = attentionSeverity(a.status) - attentionSeverity(b.status)
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
        (task.status !== "canceled" || showCanceled) &&
        (task.archived_at == null || showArchived) &&
        (group == null || columnForStatus(task.status) === group)
    )
    .sort(byFreshest)
}
