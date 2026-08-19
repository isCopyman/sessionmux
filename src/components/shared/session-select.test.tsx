import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { SessionSelect, type SessionSelectOption } from "./session-select"

const SESSIONS: SessionSelectOption[] = [
  { id: 1, title: "Fix the login bug", agent_type: "claude_code" },
  { id: 2, title: "Release notes", agent_type: "codex" },
  { id: 3, title: null, agent_type: "codex" },
]

function open(sessions = SESSIONS, props = {}) {
  const onChange = vi.fn()
  render(
    <SessionSelect
      sessions={sessions}
      value={null}
      onChange={onChange}
      placeholder="sessionPlaceholder"
      searchPlaceholder="searchSession"
      emptyLabel="noSessions"
      {...props}
    />
  )
  return { onChange, user: userEvent.setup() }
}

describe("SessionSelect", () => {
  it("lists each session as title over its agent, untitled ones as #id", async () => {
    const { user } = open()
    await user.click(screen.getByRole("button"))

    const row = screen
      .getByText("Fix the login bug")
      .closest("[data-slot=command-item]")
    expect(row?.textContent).toContain("Claude Code")
    // An untitled session is named by its only handle, the id.
    expect(screen.getByText("#3")).toBeTruthy()
  })

  it("finds a session by its title and by its #id", async () => {
    const { user } = open()
    await user.click(screen.getByRole("button"))
    const search = screen.getByPlaceholderText("searchSession")

    await user.type(search, "login")
    expect(screen.getByText("Fix the login bug")).toBeTruthy()
    expect(screen.queryByText("Release notes")).toBeNull()

    await user.clear(search)
    await user.type(search, "#2")
    expect(screen.getByText("Release notes")).toBeTruthy()
    expect(screen.queryByText("Fix the login bug")).toBeNull()
  })

  it("reports the picked session's id and closes", async () => {
    const { user, onChange } = open()
    await user.click(screen.getByRole("button"))
    await user.click(screen.getByText("Release notes"))

    expect(onChange).toHaveBeenCalledWith(2)
    expect(screen.queryByPlaceholderText("searchSession")).toBeNull()
  })

  it("shows the selected session's title on the trigger", () => {
    render(
      <SessionSelect
        sessions={SESSIONS}
        value={1}
        onChange={vi.fn()}
        placeholder="sessionPlaceholder"
      />
    )
    const trigger = screen.getByRole("button")
    expect(trigger.textContent).toContain("Fix the login bug")
    expect(trigger.getAttribute("title")).toContain("Fix the login bug")
  })

  it("never renders a still-set selection as the empty state", () => {
    // The session left the list (deleted or archived) while it was the
    // selection. The saved automation still targets id 9, so the trigger must
    // name the id rather than claim nothing is chosen.
    render(
      <SessionSelect
        sessions={SESSIONS}
        value={9}
        onChange={vi.fn()}
        placeholder="sessionPlaceholder"
      />
    )
    const trigger = screen.getByRole("button")
    expect(trigger.textContent).toContain("#9")
    expect(trigger.textContent).not.toContain("sessionPlaceholder")
  })

  it("falls back to the placeholder and stays shut while disabled", async () => {
    const onChange = vi.fn()
    render(
      <SessionSelect
        sessions={SESSIONS}
        value={null}
        onChange={onChange}
        placeholder="sessionPlaceholder"
        searchPlaceholder="searchSession"
        disabled
      />
    )
    const trigger = screen.getByRole("button")
    expect(trigger.textContent).toContain("sessionPlaceholder")

    await userEvent.setup().click(trigger)
    expect(screen.queryByPlaceholderText("searchSession")).toBeNull()
  })
})
