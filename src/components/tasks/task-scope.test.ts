import { describe, expect, it } from "vitest"
import type { WorkTask } from "@/lib/types"
import { filterTasksByScope } from "./task-scope"

function task(
  id: number,
  executionMode: WorkTask["execution_mode"],
  taskStatus: WorkTask["task_status"] = "todo"
): WorkTask {
  return {
    id,
    folder_id: 1,
    title: `t${id}`,
    config: null,
    status: "todo",
    task_status: taskStatus,
    execution_mode: executionMode,
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
    created_at: "2026-08-22T00:00:00Z",
    updated_at: "2026-08-22T00:00:00Z",
    started_at: null,
    settled_at: null,
    finished_at: null,
  }
}

const rows = [
  task(1, null),
  task(2, "manual"),
  task(3, "session"),
  task(4, "engine"),
  task(5, "session", "review"),
  task(6, "manual", "blocked"),
]

describe("filterTasksByScope", () => {
  it("keeps the three stable views as overlapping projections", () => {
    expect(filterTasksByScope(rows, "all").map((t) => t.id)).toEqual([
      1, 2, 3, 4, 5, 6,
    ])
    expect(filterTasksByScope(rows, "agent").map((t) => t.id)).toEqual([
      3, 4, 5,
    ])
    expect(filterTasksByScope(rows, "attention").map((t) => t.id)).toEqual([
      5, 6,
    ])
  })
})
