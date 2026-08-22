import { describe, expect, it, vi } from "vitest"
import type { WorkTask } from "@/lib/types"
import { buildTaskActions, type TaskActionHandlers } from "./task-actions"

function task(overrides?: Partial<WorkTask>): WorkTask {
  return {
    id: 7,
    folder_id: 1,
    title: "Fix the login flow",
    config: null,
    status: "running",
    task_status: overrides?.task_status ?? "in_progress",
    execution_mode: overrides?.execution_mode ?? "engine",
    failure_reason: null,
    last_error: null,
    run_seq: 1,
    sort_order: 1,
    worktree_folder_id: 9,
    conversation_id: 3,
    connection_id: "conn-7",
    base_branch: "main",
    base_sha: "abc",
    work_branch: "task/7",
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
    started_at: "2026-08-01T00:01:00Z",
    settled_at: null,
    finished_at: null,
    ...overrides,
  }
}

function handlers(): TaskActionHandlers {
  return {
    onManualStatus: vi.fn(),
    onStart: vi.fn(),
    onAssignSession: vi.fn(),
    onCancel: vi.fn(),
    onSubmitReview: vi.fn(),
    onRetry: vi.fn(),
    onRequeue: vi.fn(),
    onViewSession: vi.fn(),
    onMerge: vi.fn(),
    onUnqueueMerge: vi.fn(),
    onComplete: vi.fn(),
    onArchive: vi.fn(),
    onEdit: vi.fn(),
    onSchedule: vi.fn(),
  }
}

describe("buildTaskActions submit-for-review", () => {
  it("offers submit-for-review on a running card with a live connection", () => {
    const h = handlers()
    const { primary, secondaries } = buildTaskActions(task(), (key) => key, h)
    expect(primary?.label).toBe("actionCancel")
    const submit = secondaries.find((a) => a.label === "actionSubmitReview")
    expect(submit).toBeDefined()
    submit?.onClick()
    expect(h.onSubmitReview).toHaveBeenCalledTimes(1)
  })

  it("hides submit-for-review without a live connection", () => {
    const { secondaries } = buildTaskActions(
      task({ connection_id: null }),
      (key) => key,
      handlers()
    )
    expect(secondaries.some((a) => a.label === "actionSubmitReview")).toBe(
      false
    )
  })

  it("does not offer submit-for-review while awaiting input", () => {
    const { secondaries } = buildTaskActions(
      task({ status: "awaiting_input" }),
      (key) => key,
      handlers()
    )
    expect(secondaries.some((a) => a.label === "actionSubmitReview")).toBe(
      false
    )
  })
})

describe("buildTaskActions manual workflow", () => {
  it("keeps manual start separate from running an Agent", () => {
    const h = handlers()
    const { primary, secondaries } = buildTaskActions(
      task({
        status: "todo",
        task_status: "todo",
        execution_mode: null,
        conversation_id: null,
        connection_id: null,
      }),
      (key) => key,
      h
    )
    primary?.onClick()
    expect(h.onManualStatus).toHaveBeenCalledWith("in_progress")
    const agent = secondaries.find(
      (action) => action.label === "actionRunWithAgent"
    )
    agent?.onClick()
    expect(h.onStart).toHaveBeenCalledTimes(1)
    const existing = secondaries.find(
      (action) => action.label === "actionAssignSession"
    )
    existing?.onClick()
    expect(h.onAssignSession).toHaveBeenCalledTimes(1)
  })

  it("never offers engine merge controls for a manual review", () => {
    const h = handlers()
    const { primary, secondaries } = buildTaskActions(
      task({
        status: "todo",
        task_status: "review",
        execution_mode: "manual",
        conversation_id: null,
        connection_id: null,
      }),
      (key) => key,
      h
    )
    expect(primary?.label).toBe("actionComplete")
    expect(secondaries.some((action) => action.label === "actionMerge")).toBe(
      false
    )
    primary?.onClick()
    expect(h.onManualStatus).toHaveBeenCalledWith("done")
  })
})

describe("buildTaskActions persistent Session workflow", () => {
  it("opens the owning Session and never offers another engine run", () => {
    const h = handlers()
    const { primary, secondaries } = buildTaskActions(
      task({
        status: "todo",
        task_status: "in_progress",
        execution_mode: "session",
      }),
      (key) => key,
      h
    )
    expect(primary?.label).toBe("actionViewSession")
    expect(
      secondaries.some((action) => action.label === "actionRunWithAgent")
    ).toBe(false)
    secondaries
      .find((action) => action.label === "actionSubmitReview")
      ?.onClick()
    expect(h.onManualStatus).toHaveBeenCalledWith("review")
  })

  it("completes a reviewed Session task without engine merge controls", () => {
    const h = handlers()
    const { primary, secondaries } = buildTaskActions(
      task({
        status: "todo",
        task_status: "review",
        execution_mode: "session",
      }),
      (key) => key,
      h
    )
    expect(primary?.label).toBe("actionComplete")
    expect(secondaries.some((action) => action.label === "actionMerge")).toBe(
      false
    )
    primary?.onClick()
    expect(h.onManualStatus).toHaveBeenCalledWith("done")
  })
})
