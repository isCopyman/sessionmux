import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { describe, expect, it, vi } from "vitest"

import enMessages from "@/i18n/messages/en.json"
import type { DbConversationSummary, WorkTask } from "@/lib/types"
import { TaskAssignSessionDialog } from "./task-assign-session-dialog"

const task = {
  id: 7,
  title: "Check the citations",
} as WorkTask

const session = {
  id: 42,
  folder_id: 1,
  title: "Literature reviewer",
  agent_type: "claude_code",
  kind: "regular",
} as DbConversationSummary

describe("TaskAssignSessionDialog", () => {
  it("requires an explicit persistent Session before assigning", async () => {
    const onSubmit = vi.fn(async () => {})
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <TaskAssignSessionDialog
          open
          onOpenChange={() => {}}
          task={task}
          sessions={[session]}
          onSubmit={onSubmit}
        />
      </NextIntlClientProvider>
    )

    const submit = screen.getByRole("button", { name: "Assign and send" })
    expect(submit).toBeDisabled()
    await userEvent.click(
      screen.getByRole("button", { name: "Choose a session" })
    )
    await userEvent.click(screen.getByText("Literature reviewer"))
    expect(submit).toBeEnabled()
    await userEvent.click(submit)
    expect(onSubmit).toHaveBeenCalledWith(42)
  })
})
