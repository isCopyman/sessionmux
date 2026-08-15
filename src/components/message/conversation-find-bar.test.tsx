import { fireEvent, render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { describe, expect, it, vi } from "vitest"

import { ConversationFindBar } from "./conversation-find-bar"
import enMessages from "@/i18n/messages/en.json"

function renderFindBar(
  overrides: Partial<React.ComponentProps<typeof ConversationFindBar>> = {}
) {
  const props: React.ComponentProps<typeof ConversationFindBar> = {
    query: "session",
    current: 2,
    total: 5,
    searching: false,
    focusToken: 1,
    onQueryChange: vi.fn(),
    onNext: vi.fn(),
    onPrevious: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  }
  render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <ConversationFindBar {...props} />
    </NextIntlClientProvider>
  )
  return props
}

describe("ConversationFindBar", () => {
  it("focuses the query and reports the selected match", () => {
    renderFindBar()

    const input = screen.getByRole("textbox", {
      name: "Find in this session",
    })
    expect(input).toHaveFocus()
    expect(input).toHaveValue("session")
    expect(screen.getByText("2 / 5")).toBeInTheDocument()
  })

  it("updates the query and navigates with Enter or Shift+Enter", () => {
    const props = renderFindBar()
    const input = screen.getByRole("textbox")

    fireEvent.change(input, { target: { value: "history" } })
    fireEvent.keyDown(input, { key: "Enter" })
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true })

    expect(props.onQueryChange).toHaveBeenCalledWith("history")
    expect(props.onNext).toHaveBeenCalledOnce()
    expect(props.onPrevious).toHaveBeenCalledOnce()
  })

  it("closes with Escape or the close button", () => {
    const props = renderFindBar()
    const input = screen.getByRole("textbox")

    fireEvent.keyDown(input, { key: "Escape" })
    fireEvent.click(screen.getByRole("button", { name: "Close find" }))

    expect(props.onClose).toHaveBeenCalledTimes(2)
  })

  it("disables navigation and shows progress while older history loads", () => {
    renderFindBar({ query: "missing", current: 0, total: 0, searching: true })

    expect(screen.getByText("Searching…")).toBeInTheDocument()
    expect(
      screen.getByRole("button", { name: "Previous match" })
    ).toBeDisabled()
    expect(screen.getByRole("button", { name: "Next match" })).toBeDisabled()
  })
})
