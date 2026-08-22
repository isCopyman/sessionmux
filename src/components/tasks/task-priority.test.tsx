import { render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { describe, expect, it } from "vitest"
import enMessages from "@/i18n/messages/en.json"
import {
  TASK_PRIORITIES,
  TaskPriorityIcon,
  TaskPrioritySelect,
  taskPriorityRank,
} from "./task-priority"

describe("task priority", () => {
  it("keeps the five business levels in ascending order", () => {
    expect(TASK_PRIORITIES).toEqual(["none", "low", "medium", "high", "urgent"])
    expect(TASK_PRIORITIES.map(taskPriorityRank)).toEqual([0, 1, 2, 3, 4])
  })

  it("does not spend card space on the default level", () => {
    const { container, rerender } = render(<TaskPriorityIcon priority="none" />)
    expect(container.querySelector("svg")).toBeNull()
    rerender(<TaskPriorityIcon priority="urgent" />)
    expect(container.querySelectorAll("rect")).toHaveLength(4)
  })

  it("shows the current level in the shared selector", () => {
    render(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <TaskPrioritySelect value="high" onValueChange={() => {}} />
      </NextIntlClientProvider>
    )
    expect(
      screen.getByRole("combobox", { name: "Priority" })
    ).toHaveTextContent("High")
    expect(
      screen
        .getByRole("combobox", { name: "Priority" })
        .querySelectorAll("svg[data-task-priority-icon]")
    ).toHaveLength(1)
  })
})
