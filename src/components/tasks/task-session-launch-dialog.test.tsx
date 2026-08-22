import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { describe, expect, it, vi } from "vitest"

import enMessages from "@/i18n/messages/en.json"
import { describeAgentOptions } from "@/lib/api"
import type { WorkTask, WorkTaskConfig } from "@/lib/types"
import { TaskSessionLaunchDialog } from "./task-session-launch-dialog"

vi.mock("@/components/chat/agent-selector", () => ({
  AgentSelector: () => <div>Claude Code</div>,
}))

vi.mock("@/components/chat/agent-profile-selector", () => ({
  InlineAgentProfileSelector: ({
    onPendingProfileChange,
  }: {
    onPendingProfileChange?: (id: string) => Promise<boolean>
  }) => (
    <button type="button" onClick={() => void onPendingProfileChange?.("cpa")}>
      Profile: follow-default
    </button>
  ),
}))

vi.mock("@/lib/api", () => ({
  workTaskSettingsEffective: vi.fn(async () => ({
    default_agent_type: "claude_code",
    mode_id: null,
    config_values: {},
    auto_process: false,
    max_concurrent: 1,
    merge_strategy: "squash",
    auto_merge: false,
    delete_worktree_default: true,
  })),
  describeAgentOptions: vi.fn(async () => ({
    modes: null,
    config_options: [],
    available_commands: [],
  })),
}))

const task = {
  id: 7,
  folder_id: 1,
  title: "Audit the task flow",
  config: {
    prompt_blocks: [{ type: "text", text: "Audit the task flow" }],
    display_text: "Audit the task flow",
    agent_type: null,
    mode_id: null,
    config_values: {},
    label_snapshot: null,
  },
  status: "todo",
  task_status: "todo",
  execution_mode: null,
  failure_reason: null,
  last_error: null,
  run_seq: 0,
  sort_order: 0,
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
} satisfies WorkTask

describe("TaskSessionLaunchDialog", () => {
  it("uses one ordinary Session launch path for a neutral task", async () => {
    const onSubmit = vi.fn(async () => {})
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <TaskSessionLaunchDialog
          open
          onOpenChange={() => {}}
          task={task}
          folderPath="/repo"
          onSubmit={onSubmit}
        />
      </NextIntlClientProvider>
    )

    expect(
      await screen.findByRole("heading", { name: "Create a Session" })
    ).toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "Create a Worktree Session" })
    ).toBeNull()
  })

  it("keeps Profile in the explicit Session launch snapshot", async () => {
    const user = userEvent.setup()
    const onSubmit = vi.fn<(config: WorkTaskConfig) => Promise<void>>(
      async () => {}
    )
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <TaskSessionLaunchDialog
          open
          onOpenChange={() => {}}
          task={task}
          folderPath="/repo"
          onSubmit={onSubmit}
        />
      </NextIntlClientProvider>
    )

    expect(
      await screen.findByRole("heading", { name: "Create a Session" })
    ).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: /Profile:/ }))
    await user.click(screen.getByRole("button", { name: "Create and assign" }))

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    expect(onSubmit.mock.calls[0][0]).toMatchObject({
      agent_type: "claude_code",
      config_values: { __codeg_profile__: "cpa" },
    })
  })

  it("re-probes ACP selectors in the selected launch Profile", async () => {
    const user = userEvent.setup()
    vi.mocked(describeAgentOptions).mockClear()
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <TaskSessionLaunchDialog
          open
          onOpenChange={() => {}}
          task={task}
          folderPath="/repo-profile-reprobe"
          onSubmit={async () => {}}
        />
      </NextIntlClientProvider>
    )

    await waitFor(() =>
      expect(describeAgentOptions).toHaveBeenCalledWith(
        "claude_code",
        "/repo-profile-reprobe",
        "follow-default"
      )
    )
    await user.click(screen.getByRole("button", { name: /Profile:/ }))
    await waitFor(() =>
      expect(describeAgentOptions).toHaveBeenCalledWith(
        "claude_code",
        "/repo-profile-reprobe",
        "cpa"
      )
    )
  })
})
