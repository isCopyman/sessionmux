import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { SessionTimers } from "./session-timers"
import enMessages from "@/i18n/messages/en.json"

const listSessionTimers = vi.fn()
const createSessionTimer = vi.fn()
const updateSessionTimer = vi.fn()
const resetSessionTimerDelay = vi.fn()
const deleteSessionTimer = vi.fn()

vi.mock("@/lib/platform", () => ({
  subscribe: vi.fn(async () => () => {}),
  onTransportReconnect: vi.fn(() => () => {}),
}))

vi.mock("@/lib/api", () => ({
  SESSION_TIMER_CHANGED_EVENT: "session-timer://changed",
  listSessionTimers: (...args: unknown[]) => listSessionTimers(...args),
  createSessionTimer: (...args: unknown[]) => createSessionTimer(...args),
  updateSessionTimer: (...args: unknown[]) => updateSessionTimer(...args),
  resetSessionTimerDelay: (...args: unknown[]) =>
    resetSessionTimerDelay(...args),
  deleteSessionTimer: (...args: unknown[]) => deleteSessionTimer(...args),
}))

function renderTimers(conversationId: number | null = 7) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <SessionTimers conversationId={conversationId} />
    </NextIntlClientProvider>
  )
}

const timer = {
  id: "t1",
  conversationId: 7,
  idleGraceSecs: 2,
  promptText: "Read docs/current-task.md and continue",
  enabled: true,
  fireCount: 0,
  strikeCount: 0,
  createdAt: "2026-08-16T00:00:00Z",
  updatedAt: "2026-08-16T00:00:00Z",
}

describe("SessionTimers", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    listSessionTimers.mockResolvedValue([])
  })

  it("creates an idle continuation instead of a wall-clock timer", async () => {
    createSessionTimer.mockResolvedValue(timer)
    renderTimers()
    fireEvent.click(screen.getByRole("button", { name: /idle continue/i }))
    fireEvent.change(screen.getByLabelText("Continuation prompt"), {
      target: { value: "Read docs/current-task.md and continue" },
    })
    fireEvent.click(screen.getByText("Keep continuing"))

    await waitFor(() => expect(createSessionTimer).toHaveBeenCalled())
    expect(createSessionTimer.mock.calls[0][0]).toMatchObject({
      conversationId: 7,
      idleGraceSecs: 2,
      promptText: "Read docs/current-task.md and continue",
    })
  })

  it("edits, pauses, and stops the same durable timer", async () => {
    listSessionTimers.mockResolvedValue([timer])
    updateSessionTimer.mockResolvedValue(timer)
    deleteSessionTimer.mockResolvedValue(undefined)
    renderTimers()
    fireEvent.click(screen.getByRole("button", { name: /idle continue/i }))
    await screen.findByText(timer.promptText)

    fireEvent.click(screen.getByTitle("Edit"))
    fireEvent.change(screen.getAllByLabelText("Continuation prompt")[0], {
      target: { value: "Continue from PLAN.md" },
    })
    fireEvent.click(screen.getByTitle("Save"))
    await waitFor(() =>
      expect(updateSessionTimer).toHaveBeenCalledWith(
        7,
        "t1",
        expect.objectContaining({
          promptText: "Continue from PLAN.md",
          expectedUpdatedAt: timer.updatedAt,
        })
      )
    )

    fireEvent.click(screen.getByTitle("Pause"))
    await waitFor(() =>
      expect(updateSessionTimer).toHaveBeenCalledWith(
        7,
        "t1",
        expect.objectContaining({ enabled: false })
      )
    )

    fireEvent.click(screen.getByTitle("Delete"))
    await waitFor(() =>
      expect(deleteSessionTimer).toHaveBeenCalledWith(7, "t1")
    )
  })

  it("resets delay on a timer with one click", async () => {
    listSessionTimers.mockResolvedValue([timer])
    resetSessionTimerDelay.mockResolvedValue({
      ...timer,
      updatedAt: "2026-08-18T00:01:00Z",
    })
    renderTimers()
    fireEvent.click(screen.getByRole("button", { name: /idle continue/i }))
    await screen.findByText(timer.promptText)

    fireEvent.click(screen.getByTitle("Reset delay"))
    await waitFor(() =>
      expect(resetSessionTimerDelay).toHaveBeenCalledWith(7, "t1")
    )
  })

  it("offers reset delay after the reminder interval has grown", async () => {
    listSessionTimers.mockResolvedValue([{ ...timer, strikeCount: 2 }])
    resetSessionTimerDelay.mockResolvedValue({
      ...timer,
      strikeCount: 0,
      updatedAt: "2026-08-18T00:01:00Z",
    })
    renderTimers()
    fireEvent.click(screen.getByRole("button", { name: /idle continue/i }))
    expect(
      await screen.findByText(/reminder delay has grown/i)
    ).toBeInTheDocument()
    fireEvent.click(screen.getByTitle("Reset delay"))
    await waitFor(() =>
      expect(resetSessionTimerDelay).toHaveBeenCalledWith(7, "t1")
    )
  })

  it("does not render for an unpersisted draft", () => {
    renderTimers(null)
    expect(screen.queryByRole("button")).not.toBeInTheDocument()
    expect(listSessionTimers).not.toHaveBeenCalled()
  })

  it("shows the next estimated fire on the collapsed pill", async () => {
    listSessionTimers.mockResolvedValue([
      {
        ...timer,
        idleGraceSecs: 180,
        // Fired two minutes ago: 3m grace → the pill reads "~1m".
        lastFiredAt: new Date(Date.now() - 120_000).toISOString(),
      },
    ])
    renderTimers()

    const pill = await screen.findByRole("button", {
      name: /1 timer · ~1m/i,
    })
    expect(pill).toHaveAttribute(
      "title",
      expect.stringContaining("Read docs/current-task.md")
    )
  })

  it("reads 'due' instead of a frozen 0s when the estimate has passed", async () => {
    listSessionTimers.mockResolvedValue([
      {
        ...timer,
        idleGraceSecs: 60,
        // 1m grace, last fired ten minutes ago → overdue.
        lastFiredAt: new Date(Date.now() - 600_000).toISOString(),
      },
    ])
    renderTimers()
    expect(
      await screen.findByRole("button", { name: /1 timer · due/i })
    ).toBeInTheDocument()
  })

  it("keeps the plain count when every timer is paused", async () => {
    listSessionTimers.mockResolvedValue([{ ...timer, enabled: false }])
    renderTimers()
    const pill = await screen.findByRole("button", { name: /1 timer/i })
    expect(pill.textContent).not.toContain("·")
  })
})
