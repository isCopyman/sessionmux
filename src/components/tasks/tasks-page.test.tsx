import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"
import enMessages from "@/i18n/messages/en.json"
import type { FolderDetail, WorkTask, WorkTaskStatus } from "@/lib/types"
import { TasksPage } from "./tasks-page"

const h = vi.hoisted(() => ({
  tasks: [] as WorkTask[],
  viewMode: "board" as "board" | "list",
  folders: [] as FolderDetail[],
  connections: new Map<string, { status: string }>(),
  refetch: vi.fn(),
}))

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }))

vi.mock("@/contexts/tasks-view-context", () => ({
  useTasksView: () => ({
    tasks: h.tasks,
    loading: false,
    refetch: h.refetch,
    viewMode: h.viewMode,
    setViewMode: vi.fn(),
  }),
}))

vi.mock("@/stores/app-workspace-store", () => {
  const useAppWorkspaceStore = (
    selector: (state: { folders: FolderDetail[] }) => unknown
  ) => selector({ folders: h.folders })
  useAppWorkspaceStore.getState = () => ({ folders: h.folders })
  return { useAppWorkspaceStore }
})

vi.mock("@/contexts/acp-connections-context", () => ({
  useConnectionStore: () => ({
    getConnection: (key: string) => h.connections.get(key),
    getActiveKey: () => null,
    subscribeKey: () => () => {},
    subscribeActiveKey: () => () => {},
  }),
}))

vi.mock("./task-editor-dialog", () => ({ TaskEditorDialog: () => null }))
vi.mock("./task-detail-sheet", () => ({ TaskDetailSheet: () => null }))
vi.mock("./task-merge-dialog", () => ({ TaskMergeDialog: () => null }))
vi.mock("./task-complete-dialog", () => ({ TaskCompleteDialog: () => null }))
vi.mock("./task-cancel-dialog", () => ({ TaskCancelDialog: () => null }))
vi.mock("./task-restart-dialog", () => ({ TaskRestartDialog: () => null }))
vi.mock("./task-schedule-dialog", () => ({ TaskScheduleDialog: () => null }))
vi.mock("./task-settings-dialog", () => ({ TaskSettingsDialog: () => null }))
vi.mock("./task-transcript-dialog", () => ({
  TaskTranscriptDialog: () => null,
}))

function folder(id: number, name: string): FolderDetail {
  return {
    id,
    name,
    path: `/${name}`,
    git_branch: "main",
    default_agent_type: null,
    last_opened_at: "2026-08-01T00:00:00Z",
    sort_order: id,
    color: "",
    parent_id: null,
    kind: "regular",
    alias: null,
  }
}

function task(
  id: number,
  status: WorkTaskStatus,
  extra?: Partial<WorkTask>
): WorkTask {
  return {
    id,
    folder_id: 1,
    title: `t${id}`,
    config: null,
    status,
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
    updated_at: "2026-08-01T00:00:00Z",
    started_at: null,
    settled_at: null,
    finished_at: null,
    ...extra,
  }
}

function renderPage() {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <TasksPage />
    </NextIntlClientProvider>
  )
}

function columnRoot(name: string) {
  const heading = screen.getByRole("heading", { name })
  const root = heading.parentElement?.parentElement
  if (!root) throw new Error(`no column root for ${name}`)
  return root
}

function expectTitleOrder(root: HTMLElement, titles: string[]) {
  const text = root.textContent ?? ""
  const positions = titles.map((title) => text.indexOf(title))
  expect(positions.every((p) => p >= 0)).toBe(true)
  expect(positions).toEqual([...positions].sort((a, b) => a - b))
}

function headerLabels() {
  return screen.getAllByTestId("task-group-header").map((el) => el.textContent)
}

beforeEach(() => {
  localStorage.clear()
  h.tasks = []
  h.viewMode = "board"
  h.folders = [folder(1, "alpha"), folder(2, "bravo")]
  h.connections.clear()
  h.refetch.mockReset()
})

describe("TasksPage grouping", () => {
  it("shows no group headers when grouping is none — four columns still there", () => {
    h.tasks = [
      task(1, "todo", { folder_id: 1, title: "todo-alpha" }),
      task(2, "todo", { folder_id: 2, title: "todo-bravo" }),
      task(3, "running", { folder_id: 1, title: "run-alpha" }),
      task(4, "review", { folder_id: 2, title: "review-bravo" }),
      task(5, "done", { folder_id: 1, title: "done-alpha" }),
    ]
    renderPage()

    expect(screen.queryByTestId("task-group-header")).toBeNull()
    expect(screen.getByRole("heading", { name: "To do" })).toBeInTheDocument()
    expect(
      screen.getByRole("heading", { name: "In progress" })
    ).toBeInTheDocument()
    expect(
      screen.getByRole("heading", { name: "Needs you" })
    ).toBeInTheDocument()
    expect(screen.getByRole("heading", { name: "Done" })).toBeInTheDocument()
    expect(screen.getByText("todo-alpha")).toBeInTheDocument()
    expect(screen.getByText("todo-bravo")).toBeInTheDocument()
  })

  it("segments each column by project with the right counts", () => {
    localStorage.setItem("workspace:tasks-board-grouping", "folder")
    h.tasks = [
      task(1, "todo", { folder_id: 1, title: "todo-alpha" }),
      task(2, "todo", { folder_id: 2, title: "todo-bravo" }),
      task(3, "todo", { folder_id: 1, title: "todo-alpha-2" }),
      task(4, "running", { folder_id: 2, title: "run-bravo" }),
      task(5, "review", { folder_id: 1, title: "review-alpha" }),
      task(6, "done", { folder_id: 2, title: "done-bravo" }),
    ]
    renderPage()

    expect(screen.getByRole("heading", { name: "To do" })).toBeInTheDocument()
    expect(
      screen.getByRole("heading", { name: "In progress" })
    ).toBeInTheDocument()
    expect(
      screen.getByRole("heading", { name: "Needs you" })
    ).toBeInTheDocument()
    expect(screen.getByRole("heading", { name: "Done" })).toBeInTheDocument()

    const todo = columnRoot("To do")
    expect(
      within(todo)
        .getAllByTestId("task-group-header")
        .map((el) => el.textContent)
    ).toEqual(["alpha2", "bravo1"])
    expectTitleOrder(todo, ["todo-alpha", "todo-alpha-2", "todo-bravo"])

    expect(
      within(columnRoot("In progress")).getByTestId("task-group-header")
        .textContent
    ).toBe("bravo1")
    expect(
      within(columnRoot("Needs you")).getByTestId("task-group-header")
        .textContent
    ).toBe("alpha1")
    expect(
      within(columnRoot("Done")).getByTestId("task-group-header").textContent
    ).toBe("bravo1")
  })

  it("segments by agent and parks tasks without one as Ungrouped", () => {
    localStorage.setItem("workspace:tasks-board-grouping", "agent")
    h.tasks = [
      task(1, "todo", { agent_type: "codex", title: "codex-todo" }),
      task(2, "todo", { agent_type: "claude_code", title: "claude-todo" }),
      task(3, "todo", { title: "no-agent-todo" }),
    ]
    renderPage()

    const todo = columnRoot("To do")
    expect(
      within(todo)
        .getAllByTestId("task-group-header")
        .map((el) => el.textContent)
    ).toEqual(["Claude Code1", "Codex1", "Ungrouped1"])
    expectTitleOrder(todo, ["claude-todo", "codex-todo", "no-agent-todo"])
  })

  it("hides project group headers once a single folder is the filter", async () => {
    localStorage.setItem("workspace:tasks-board-grouping", "folder")
    h.tasks = [
      task(1, "todo", { folder_id: 1, title: "todo-alpha" }),
      task(2, "todo", { folder_id: 2, title: "todo-bravo" }),
    ]
    renderPage()
    expect(screen.getAllByTestId("task-group-header").length).toBeGreaterThan(0)

    await userEvent.click(screen.getByRole("button", { name: "All folders" }))
    await userEvent.click(await screen.findByRole("option", { name: /alpha/ }))

    expect(screen.queryByTestId("task-group-header")).toBeNull()
    expect(screen.getByText("todo-alpha")).toBeInTheDocument()
    expect(screen.queryByText("todo-bravo")).toBeNull()
  })

  it("writes the grouping choice to localStorage and restores it", async () => {
    h.tasks = [
      task(1, "todo", { folder_id: 1, title: "todo-alpha" }),
      task(2, "todo", { folder_id: 2, title: "todo-bravo" }),
    ]
    const { unmount } = renderPage()
    expect(screen.queryByTestId("task-group-header")).toBeNull()

    await userEvent.click(screen.getByRole("combobox", { name: /^Group:/ }))
    await userEvent.click(screen.getByRole("option", { name: "By project" }))

    expect(localStorage.getItem("workspace:tasks-board-grouping")).toBe(
      "folder"
    )
    expect(headerLabels().length).toBeGreaterThan(0)

    unmount()
    renderPage()
    expect(headerLabels().length).toBeGreaterThan(0)
    expect(screen.getByRole("combobox", { name: /^Group:/ })).toHaveTextContent(
      "By project"
    )
  })

  it("adds the same group headers in list view", () => {
    localStorage.setItem("workspace:tasks-board-grouping", "folder")
    h.viewMode = "list"
    h.tasks = [
      task(1, "todo", {
        folder_id: 1,
        title: "todo-alpha",
        updated_at: "2026-08-01T02:00:00Z",
      }),
      task(2, "todo", {
        folder_id: 2,
        title: "todo-bravo",
        updated_at: "2026-08-01T01:00:00Z",
      }),
    ]
    renderPage()

    expect(
      screen.getAllByTestId("task-group-header").map((el) => el.textContent)
    ).toEqual(["alpha1", "bravo1"])
    expect(screen.queryByRole("heading", { name: "To do" })).toBeNull()
    expect(screen.getByText("todo-alpha")).toBeInTheDocument()
    expect(screen.getByText("todo-bravo")).toBeInTheDocument()
  })
})

describe("TasksPage attention column", () => {
  it("renders failed then awaiting_input then review then merging", () => {
    h.tasks = [
      task(1, "merging", {
        title: "Merging one",
        updated_at: "2026-08-01T08:00:00Z",
      }),
      task(2, "review", {
        title: "Review me",
        updated_at: "2026-08-01T07:00:00Z",
      }),
      task(3, "awaiting_input", {
        title: "Ask me",
        updated_at: "2026-08-01T06:00:00Z",
      }),
      task(4, "failed", {
        title: "Broke",
        updated_at: "2026-08-01T01:00:00Z",
      }),
    ]
    renderPage()
    expectTitleOrder(columnRoot("Needs you"), [
      "Broke",
      "Ask me",
      "Review me",
      "Merging one",
    ])
  })
})

describe("TasksPage activity dots", () => {
  it("pulses a running task whose session is Prompting", () => {
    h.tasks = [
      task(1, "running", {
        title: "live",
        conversation_id: 101,
        connection_id: "conversation:101",
        folder_id: 1,
        agent_type: "codex",
      }),
    ]
    h.connections.set("conversation:101", { status: "prompting" })
    renderPage()
    const dot = screen.getByTestId("task-activity-dot")
    expect(dot).toHaveAttribute("data-pulse", "true")
    expect(dot).toHaveClass("animate-pulse")
  })

  it("draws a still dot when running but not Prompting", () => {
    h.tasks = [
      task(1, "running", {
        title: "waiting",
        conversation_id: 101,
        connection_id: "conversation:101",
        folder_id: 1,
        agent_type: "codex",
      }),
    ]
    h.connections.set("conversation:101", { status: "connected" })
    renderPage()
    const dot = screen.getByTestId("task-activity-dot")
    expect(dot).toHaveAttribute("data-pulse", "false")
    expect(dot).not.toHaveClass("animate-pulse")
  })

  it("does not render a dot without a conversation id", () => {
    h.tasks = [
      task(1, "running", {
        title: "no-session",
        conversation_id: null,
        agent_type: "codex",
      }),
    ]
    h.connections.set("conversation:101", { status: "prompting" })
    renderPage()
    expect(screen.queryByTestId("task-activity-dot")).toBeNull()
  })
})
