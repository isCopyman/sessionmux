import type { TasksBoardGrouping } from "@/lib/tasks-board-filter-storage"
import type { AgentType, WorkTask } from "@/lib/types"

const UNGROUPED_KEY = "__ungrouped__"

/** One column-internal (or list) segment. `label` is already resolved to a
 *  display string; `ungrouped` is the fallback bucket (no project name, or
 *  no agent) and the caller substitutes the i18n "Ungrouped" string. */
export interface TaskBoardSegment {
  key: string
  label: string | null
  ungrouped: boolean
  tasks: WorkTask[]
}

export interface SegmentTasksOptions {
  folderNames: ReadonlyMap<number, string>
  sessionNames: ReadonlyMap<number, string>
  /** When grouping by project and a single folder is already the page
   *  filter, the column collapses to one unlabeled segment — no extra
   *  header over a filter the user just chose. */
  folderFilter: number | null
  agentLabel: (agentType: AgentType) => string
}

/** Headers are hidden for grouping=none, and for project grouping once
 *  `folderFilter` has already narrowed the board to one project. Agent
 *  grouping always shows headers (even a single agent). */
export function groupingShowsHeaders(
  grouping: TasksBoardGrouping,
  folderFilter: number | null
): boolean {
  if (grouping === "none") return false
  if (grouping === "folder" && folderFilter != null) return false
  return true
}

/**
 * Split an already-sorted column (or list) into grouping segments. Input
 * order is preserved inside each segment; segments themselves sort by
 * display label, with the ungrouped bucket last.
 *
 * Does not change which tasks belong to which board column — that stays
 * `columnForStatus`. Call this on one column's (or the list's) rows.
 */
export function segmentTasksForGrouping(
  tasks: WorkTask[],
  grouping: TasksBoardGrouping,
  opts: SegmentTasksOptions
): TaskBoardSegment[] {
  if (grouping === "none") {
    return [{ key: "all", label: null, ungrouped: false, tasks }]
  }
  if (grouping === "folder" && opts.folderFilter != null) {
    return [{ key: "all", label: null, ungrouped: false, tasks }]
  }

  const buckets = new Map<string, TaskBoardSegment>()
  const firstSeen: string[] = []

  for (const task of tasks) {
    const placed = placeTask(task, grouping, opts)
    let bucket = buckets.get(placed.key)
    if (!bucket) {
      bucket = {
        key: placed.key,
        label: placed.label,
        ungrouped: placed.ungrouped,
        tasks: [],
      }
      buckets.set(placed.key, bucket)
      firstSeen.push(placed.key)
    }
    bucket.tasks.push(task)
  }

  return firstSeen
    .map((key) => buckets.get(key)!)
    .sort((a, b) => {
      if (a.ungrouped !== b.ungrouped) return a.ungrouped ? 1 : -1
      return (a.label ?? "").localeCompare(b.label ?? "")
    })
}

function placeTask(
  task: WorkTask,
  grouping: TasksBoardGrouping,
  opts: SegmentTasksOptions
): { key: string; label: string | null; ungrouped: boolean } {
  if (grouping === "folder") {
    const name = opts.folderNames.get(task.folder_id) ?? null
    if (name) {
      return { key: `folder:${task.folder_id}`, label: name, ungrouped: false }
    }
    return { key: UNGROUPED_KEY, label: null, ungrouped: true }
  }

  if (grouping === "session") {
    const id = task.conversation_id
    const name = id == null ? null : (opts.sessionNames.get(id) ?? null)
    if (id != null) {
      return {
        key: `session:${id}`,
        label: name ?? `Session #${id}`,
        ungrouped: false,
      }
    }
    return { key: UNGROUPED_KEY, label: null, ungrouped: true }
  }

  const agentType = task.agent_type ?? task.config?.agent_type ?? null
  if (agentType) {
    return {
      key: `agent:${agentType}`,
      label: opts.agentLabel(agentType),
      ungrouped: false,
    }
  }
  return { key: UNGROUPED_KEY, label: null, ungrouped: true }
}
