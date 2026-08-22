import { describe, expect, it } from "vitest"
import type { WorkTask, WorkTaskStatus } from "@/lib/types"
import { connectionKeysForTask, taskActivityDot } from "./task-activity"

function task(status: WorkTaskStatus, extra?: Partial<WorkTask>): WorkTask {
  return {
    id: 1,
    folder_id: 7,
    title: "t",
    config: null,
    status,
    task_status: extra?.task_status ?? "in_progress",
    execution_mode: extra?.execution_mode ?? "engine",
    failure_reason: null,
    last_error: null,
    run_seq: 0,
    sort_order: 1,
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
    updated_at: "2026-08-01T00:00:00Z",
    started_at: null,
    settled_at: null,
    finished_at: null,
    ...extra,
  }
}

describe("connectionKeysForTask", () => {
  it("returns no keys without a conversation id", () => {
    expect(
      connectionKeysForTask(
        task("running", { conversation_id: null, connection_id: "live-1" })
      )
    ).toEqual([])
  })

  it("prefers the live connection id then the canonical conv key", () => {
    expect(
      connectionKeysForTask(
        task("running", {
          conversation_id: 101,
          connection_id: "conversation:101",
          folder_id: 7,
          agent_type: "codex",
        })
      )
    ).toEqual(["conversation:101", "conv-7-codex-101"])
  })

  it("falls back to connection_id alone when the agent is unknown", () => {
    expect(
      connectionKeysForTask(
        task("running", {
          conversation_id: 101,
          connection_id: "live-tab",
        })
      )
    ).toEqual(["live-tab"])
  })
})

describe("taskActivityDot", () => {
  it("pulses when running and the connection is Prompting", () => {
    expect(
      taskActivityDot(task("running", { conversation_id: 1 }), "prompting")
    ).toBe("pulse")
  })

  it("is still when running but not Prompting", () => {
    expect(
      taskActivityDot(task("running", { conversation_id: 1 }), "connected")
    ).toBe("static")
    expect(
      taskActivityDot(task("running", { conversation_id: 1 }), undefined)
    ).toBe("static")
  })

  it("renders nothing without a conversation id, even if Prompting", () => {
    expect(taskActivityDot(task("running"), "prompting")).toBeNull()
  })

  it("renders nothing when the task is not running", () => {
    expect(
      taskActivityDot(
        task("awaiting_input", { conversation_id: 1 }),
        "prompting"
      )
    ).toBeNull()
    expect(
      taskActivityDot(task("todo", { conversation_id: 1 }), "prompting")
    ).toBeNull()
  })
})
