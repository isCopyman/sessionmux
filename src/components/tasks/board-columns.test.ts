import { describe, expect, it } from "vitest"
import {
  ALL_WORK_TASK_BUSINESS_STATUSES,
  BOARD_COLUMN_IDS,
  columnForStatus,
  filterTasksForList,
  groupTasksByColumn,
  statusForColumn,
} from "./board-columns"
import { TASKS_STATUS_GROUPS } from "@/lib/tasks-board-filter-storage"
import type { WorkTask, WorkTaskBusinessStatus } from "@/lib/types"

const statuses: WorkTaskBusinessStatus[] = [
  "backlog",
  "todo",
  "in_progress",
  "review",
  "done",
  "blocked",
  "canceled",
]

function task(
  id: number,
  task_status: WorkTaskBusinessStatus,
  extra: Partial<WorkTask> = {}
): WorkTask {
  return {
    id,
    folder_id: 1,
    title: `t${id}`,
    config: null,
    status: "todo",
    task_status,
    execution_mode: null,
    failure_reason: null,
    last_error: null,
    run_seq: 0,
    sort_order: id,
    worktree_folder_id: null,
    conversation_id: null,
    connection_id: null,
    base_branch: null,
    base_sha: null,
    work_branch: null,
    cleanup_state: null,
    verdict: null,
    result_summary: null,
    files_changed: null,
    additions: null,
    deletions: null,
    merge_commit: null,
    preflight: null,
    archived_at: null,
    scheduled_at: null,
    created_at: "2026-08-01T00:00:00Z",
    updated_at: `2026-08-01T0${id}:00:00Z`,
    started_at: null,
    settled_at: null,
    finished_at: null,
    ...extra,
  }
}

describe("seven-column task board", () => {
  it("maps every status one-to-one and round-trips", () => {
    for (const status of statuses) {
      expect(statusForColumn(columnForStatus(status))).toBe(status)
    }
    expect(ALL_WORK_TASK_BUSINESS_STATUSES).toEqual(statuses)
    expect(TASKS_STATUS_GROUPS).toEqual(BOARD_COLUMN_IDS)
  })

  it("puts every card in its own column", () => {
    const grouped = groupTasksByColumn(
      statuses.map((status, index) => task(index + 1, status))
    )
    for (const status of statuses) {
      expect(
        grouped[columnForStatus(status)].map((row) => row.task_status)
      ).toEqual([status])
    }
  })

  it("keeps canceled cards visible and archived cards optional", () => {
    const rows = [
      task(1, "canceled"),
      task(2, "done", { archived_at: "2026-08-02T00:00:00Z" }),
    ]
    expect(groupTasksByColumn(rows).canceled).toHaveLength(1)
    expect(groupTasksByColumn(rows).done).toHaveLength(0)
    expect(groupTasksByColumn(rows, true).done).toHaveLength(1)
  })

  it("sorts columns and list freshest first without mutating input", () => {
    const rows = [task(1, "todo"), task(3, "todo"), task(2, "review")]
    expect(groupTasksByColumn(rows).todo.map((row) => row.id)).toEqual([3, 1])
    expect(filterTasksForList(rows, null, false).map((row) => row.id)).toEqual([
      3, 2, 1,
    ])
    expect(rows.map((row) => row.id)).toEqual([1, 3, 2])
  })

  it("filters the list by the exact visible column", () => {
    const rows = [task(1, "review"), task(2, "blocked"), task(3, "done")]
    expect(
      filterTasksForList(rows, "review", false).map((row) => row.id)
    ).toEqual([1])
    expect(
      filterTasksForList(rows, "blocked", false).map((row) => row.id)
    ).toEqual([2])
  })
})
