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
  state: QueuedMessage["state"] = "queued"
): QueuedMessage {
  return {
    id,
    draft: {
      blocks: [{ type: "text", text: id }],
      displayText: id,
    },
    modeId: null,
    state,
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
})
