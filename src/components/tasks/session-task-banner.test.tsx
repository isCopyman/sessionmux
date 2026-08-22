import { fireEvent, render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import enMessages from "@/i18n/messages/en.json"
import type { WorkTask } from "@/lib/types"
import { consumePendingTaskDetail } from "@/lib/task-compose-events"
import { SessionTaskBanner } from "./session-task-banner"

const projection = vi.hoisted(() => ({
  tasks: [] as WorkTask[],
  openTaskBoard: vi.fn(() => true),
}))

vi.mock("@/contexts/tasks-view-context", () => ({
  useOptionalTasksView: () => ({ tasks: projection.tasks }),
}))

vi.mock("@/lib/open-task-board", () => ({
  useOpenTaskBoard: () => projection.openTaskBoard,
}))

function task(overrides: Partial<WorkTask> = {}): WorkTask {
  return {
    id: 21,
    folder_id: 1,
    title: "Review task projection",
    config: null,
    status: "running",
    task_status: "in_progress",
    execution_mode: "session",
    failure_reason: null,
    last_error: null,
    run_seq: 0,
    sort_order: 0,
    worktree_folder_id: null,
    conversation_id: 7,
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
    updated_at: "2026-08-22T00:00:00Z",
    started_at: null,
    settled_at: null,
    finished_at: null,
    ...overrides,
  }
}

function renderBanner(conversationId: number | null = 7) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <SessionTaskBanner conversationId={conversationId} />
    </NextIntlClientProvider>
  )
}

describe("SessionTaskBanner", () => {
  beforeEach(() => {
    projection.tasks = []
    projection.openTaskBoard.mockReset()
    consumePendingTaskDetail()
  })

  it("stays hidden when this Session has no active responsibility", () => {
    projection.tasks = [
      task({ conversation_id: 8 }),
      task({ task_status: "done" }),
      task({ id: 23, archived_at: "2026-08-22T00:00:00Z" }),
    ]
    const { container } = renderBanner()
    expect(container.querySelector("[data-session-task-banner]")).toBeNull()
  })

  it("opens an independent task panel and navigates to the exact board card", () => {
    projection.tasks = [
      task(),
      task({ id: 22, title: "Waiting for review", task_status: "review" }),
      task({ id: 24, title: "Another Session", conversation_id: 8 }),
    ]
    renderBanner()

    fireEvent.click(screen.getByRole("button", { name: /To-dos 2/ }))
    expect(screen.getByRole("dialog")).toHaveTextContent("Waiting for review")
    expect(screen.queryByText("Another Session")).toBeNull()

    fireEvent.click(screen.getByRole("button", { name: /Waiting for review/ }))
    expect(projection.openTaskBoard).toHaveBeenCalledWith("project:1")
    expect(consumePendingTaskDetail()).toBe(22)
  })
})
