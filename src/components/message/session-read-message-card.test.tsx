import { type ReactElement } from "react"
import { render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { describe, expect, it, vi } from "vitest"

import enMessages from "@/i18n/messages/en.json"
import { SessionReadMessageCard } from "./session-read-message-card"

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
      conversations: [{ id: 290, title: "Session C", agent_type: "codex" }],
    }),
}))

function renderWithIntl(ui: ReactElement) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      {ui}
    </NextIntlClientProvider>
  )
}

describe("SessionReadMessageCard", () => {
  it("renders the opened letter as a prompt-style card with subject and body", () => {
    renderWithIntl(
      <SessionReadMessageCard
        letterKey="tool-read-1"
        output={JSON.stringify({
          event_id: "b80f5bea-2dd6-41b5-8a07-b68d49fe269a",
          from_session_id: 290,
          from_title: "Session C",
          title: "Need review",
          body: "please check claim 3",
          expects_reply: true,
        })}
      />
    )

    expect(screen.getByText("From")).toBeInTheDocument()
    expect(screen.getByText("Session C")).toBeInTheDocument()
    expect(document.querySelector("[data-session-mail-card='inbound']")).not.toBeNull()
    expect(screen.getByText("Need review")).toBeInTheDocument()
    expect(screen.getByText("please check claim 3")).toBeInTheDocument()
    expect(screen.getByText("Read, awaiting reply")).toBeInTheDocument()
    expect(document.querySelector("[data-session-read-message]")).not.toBeNull()
    expect(
      document.querySelector("[data-collaboration-subject]")?.textContent
    ).toBe("Need review")
  })
})
