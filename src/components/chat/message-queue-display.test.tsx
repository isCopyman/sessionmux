import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

vi.mock("motion/react", () => ({
  Reorder: {
    Group: ({ children }: { children: React.ReactNode }) => (
      <div>{children}</div>
    ),
    Item: ({ children }: { children: React.ReactNode }) => (
      <div>{children}</div>
    ),
  },
  useDragControls: () => ({ start: vi.fn() }),
}))

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, string>) =>
    values?.reason ? `${key}:${values.reason}` : key,
}))

import type { QueuedMessage } from "@/hooks/use-message-queue"
import { MessageQueueDisplay } from "./message-queue-display"

function item(
  id: string,
  state: QueuedMessage["state"] = "queued",
  source: QueuedMessage["source"] = "user"
): QueuedMessage {
  return {
    id,
    draft: {
      blocks: [{ type: "text", text: id }],
      displayText: id,
    },
    taskId: null,
    modeId: null,
    state,
    source,
    attempts: state === "queued" ? 0 : 1,
    pausedReason: state === "paused" ? "failed" : null,
  }
}

describe("MessageQueueDisplay", () => {
  it("offers an explicit resume after cancellation paused queued work", () => {
    const onResume = vi.fn()
    render(
      <MessageQueueDisplay
        queue={[item("later")]}
        pausedReason="cancelled_current_turn"
        onResume={onResume}
        onRetry={() => {}}
        onReorder={() => {}}
        onEdit={() => {}}
        onDelete={() => {}}
        editingItemId={null}
      />
    )

    expect(screen.getByText("paused:pauseReasonCancelled")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "resumeQueue" }))
    expect(onResume).toHaveBeenCalledTimes(1)
  })

  it("labels non-user entries with their scheduling class", () => {
    render(
      <MessageQueueDisplay
        queue={[item("mine"), item("auto", "queued", "timer")]}
        pausedReason={null}
        onResume={() => {}}
        onRetry={() => {}}
        onReorder={() => {}}
        onEdit={() => {}}
        onDelete={() => {}}
        editingItemId={null}
      />
    )

    expect(screen.getByText("sourceTimer")).toBeInTheDocument()
    expect(screen.queryByText("sourceUser")).not.toBeInTheDocument()
  })

  it("retries a failed item and locks a claimed item against mutation", () => {
    const onRetry = vi.fn()
    const onEdit = vi.fn()
    const onDelete = vi.fn()
    render(
      <MessageQueueDisplay
        queue={[item("failed", "paused"), item("sending", "claimed")]}
        pausedReason="mode rejected"
        onResume={() => {}}
        onRetry={onRetry}
        onReorder={() => {}}
        onEdit={onEdit}
        onDelete={onDelete}
        editingItemId={null}
      />
    )

    fireEvent.click(screen.getByTitle("retryItem"))
    expect(onRetry).toHaveBeenCalledWith("failed")
    const editButtons = screen.getAllByTitle("editItem")
    const deleteButtons = screen.getAllByTitle("deleteItem")
    expect(editButtons[1]).toBeDisabled()
    expect(deleteButtons[1]).toBeDisabled()
  })

  it("keeps a task-owned brief immutable from the generic queue controls", () => {
    const taskItem = item("task", "queued", "task")
    taskItem.taskId = 42
    render(
      <MessageQueueDisplay
        queue={[taskItem]}
        pausedReason={null}
        onResume={() => {}}
        onRetry={() => {}}
        onReorder={() => {}}
        onEdit={() => {}}
        onDelete={() => {}}
        editingItemId={null}
      />
    )

    expect(screen.getByText("sourceTask")).toBeInTheDocument()
    expect(screen.queryByTitle("editItem")).toBeNull()
    expect(screen.queryByTitle("deleteItem")).toBeNull()
  })
})
