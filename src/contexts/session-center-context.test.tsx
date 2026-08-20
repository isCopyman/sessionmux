import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import {
  SessionCenterProvider,
  useSessionCenter,
} from "./session-center-context"

const captured = vi.hoisted(() => ({
  mounted: false,
  initialSearch: undefined as string | undefined,
}))

vi.mock("@/components/conversations/conversation-manage-dialog", () => ({
  ConversationManageDialog: ({
    onOpenChange,
    initialSearch,
  }: {
    onOpenChange: (open: boolean) => void
    initialSearch?: string
  }) => {
    captured.mounted = true
    captured.initialSearch = initialSearch
    return (
      <button onClick={() => onOpenChange(false)}>close session center</button>
    )
  },
}))

function Harness() {
  const { openSessionCenter } = useSessionCenter()
  return (
    <>
      <button onClick={() => openSessionCenter({ search: "auth" })}>
        open with query
      </button>
      <button onClick={() => openSessionCenter()}>open plain</button>
    </>
  )
}

function renderHarness() {
  render(
    <SessionCenterProvider>
      <Harness />
    </SessionCenterProvider>
  )
  return userEvent.setup()
}

describe("SessionCenterProvider", () => {
  beforeEach(() => {
    captured.mounted = false
    captured.initialSearch = undefined
  })

  it("carries an entry point's search term into the dialog", async () => {
    const user = renderHarness()
    expect(captured.mounted).toBe(false)

    await user.click(screen.getByRole("button", { name: "open with query" }))

    expect(captured.initialSearch).toBe("auth")
  })

  it("does not leak the last visit's search into the next one", async () => {
    const user = renderHarness()

    await user.click(screen.getByRole("button", { name: "open with query" }))
    await user.click(
      screen.getByRole("button", { name: "close session center" })
    )
    await user.click(screen.getByRole("button", { name: "open plain" }))

    expect(captured.initialSearch).toBe("")
  })
})
