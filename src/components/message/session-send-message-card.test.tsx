import { type ReactElement } from "react"
import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import enMessages from "@/i18n/messages/en.json"
import { copyTextToClipboard } from "@/lib/utils"
import { SessionSendMessageCard } from "./session-send-message-card"

vi.mock("@/lib/utils", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/utils")>("@/lib/utils")
  return { ...actual, copyTextToClipboard: vi.fn().mockResolvedValue(true) }
})

// SessionRoomChip (now inside SessionMailCard) pulls open-room -> tab-store,
// whose module init reads the real app-workspace store; this suite stubs that
// store, so stub the chip out of the import graph.
vi.mock("@/components/message/session-room-chip", () => ({
  SessionRoomChip: () => null,
}))
vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => ({ openTab: vi.fn() }),
}))

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (
    selector: (state: {
      conversations: Array<{
        id: number
        title: string
        agent_type: string
      }>
    }) => unknown
  ) =>
    selector({
      conversations: [{ id: 291, title: "Session D", agent_type: "codex" }],
    }),
}))

function renderWithIntl(ui: ReactElement) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      {ui}
    </NextIntlClientProvider>
  )
}

const mockCopy = vi.mocked(copyTextToClipboard)

describe("SessionSendMessageCard", () => {
  beforeEach(() => {
    mockCopy.mockClear()
    mockCopy.mockResolvedValue(true)
  })

  it("copies the letter body from the action next to the envelope", async () => {
    renderWithIntl(
      <SessionSendMessageCard
        letterKey="tool-1"
        input={JSON.stringify({
          target_session_ids: [291],
          content: "ping Session D",
        })}
        output={JSON.stringify({
          event_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        })}
      />
    )

    fireEvent.click(screen.getByRole("button", { name: "Copy" }))

    await waitFor(() => {
      expect(mockCopy).toHaveBeenCalledWith("ping Session D")
    })
    expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy()
    expect(screen.getByText("Sent letter")).toBeInTheDocument()
    expect(screen.getByText("To")).toBeInTheDocument()
    expect(
      document.querySelector("[data-session-mail-card='outbound']")
    ).not.toBeNull()
    expect(
      screen.getByRole("button", { name: "Show raw MCP tools" })
    ).toBeTruthy()
  })

  it("keeps letters on the agent side and labels FYI mail as needing no reply", () => {
    renderWithIntl(
      <SessionSendMessageCard
        letterKey="tool-2"
        input={JSON.stringify({
          target_session_ids: [291],
          content: "FYI: deploy done",
          expects_reply: false,
        })}
      />
    )

    const card = document.querySelector("[data-session-mail-card='outbound']")
    expect(card).not.toBeNull()
    // The right column is reserved for what the human typed.
    expect(card?.className).toContain("self-start")
    expect(card?.className).not.toContain("self-end")
    expect(screen.getByText("No reply needed")).toBeInTheDocument()
  })
})
