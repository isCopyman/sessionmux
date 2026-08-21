import {
  render,
  screen,
  waitFor,
  cleanup,
  fireEvent,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import type { ComponentProps } from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { ClaudeProfileCatalog } from "./claude-profile-catalog"
import enMessages from "@/i18n/messages/en.json"
import {
  FOLLOW_DEFAULT_CLAUDE_PROFILE_ID,
  isValidClaudeProfileId,
  type ClaudeProfileInfo,
} from "@/lib/types"

const api = vi.hoisted(() => ({
  claudeProfileList: vi.fn(),
  claudeProfileUpsert: vi.fn(),
  claudeProfileDelete: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  claudeProfileList: (...args: unknown[]) => api.claudeProfileList(...args),
  claudeProfileUpsert: (...args: unknown[]) => api.claudeProfileUpsert(...args),
  claudeProfileDelete: (...args: unknown[]) => api.claudeProfileDelete(...args),
}))

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

const FOLLOW: ClaudeProfileInfo = {
  id: "follow-default",
  label: "Follow default",
  kind: "followDefault",
  authTokenMasked: "",
  createdAt: "1970-01-01T00:00:00Z",
  updatedAt: "1970-01-01T00:00:00Z",
}

const RELAY: ClaudeProfileInfo = {
  id: "api",
  label: "中转",
  kind: "managed",
  baseUrl: "https://example.test/v1",
  authTokenMasked: "sk-t••••••••7890",
  createdAt: "2026-08-21T00:00:00+00:00",
  updatedAt: "2026-08-21T00:00:00+00:00",
}

function renderCatalog(
  props?: Partial<ComponentProps<typeof ClaudeProfileCatalog>>
) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <ClaudeProfileCatalog
        defaultProfileId=""
        onSetAgentDefault={vi.fn(async () => {})}
        {...props}
      />
    </NextIntlClientProvider>
  )
}

/** Wait for the first list to settle: the tab strip replaces the spinner. */
function addProfileButton() {
  return screen.findByRole("button", { name: "Add profile" })
}

/** `+` opens a menu (duplicate / blank); every add goes through it. */
async function addBlankProfile(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await addProfileButton())
  await user.click(
    await screen.findByRole("menuitem", { name: "Blank profile" })
  )
}

describe("isValidClaudeProfileId", () => {
  it("rejects illegal values and the reserved id", () => {
    expect(isValidClaudeProfileId("work")).toBe(true)
    expect(isValidClaudeProfileId("a")).toBe(true)
    expect(isValidClaudeProfileId("a".repeat(64))).toBe(true)
    expect(isValidClaudeProfileId("")).toBe(false)
    expect(isValidClaudeProfileId("a".repeat(65))).toBe(false)
    expect(isValidClaudeProfileId("Work")).toBe(false)
    expect(isValidClaudeProfileId("has space")).toBe(false)
    expect(isValidClaudeProfileId("has/slash")).toBe(false)
    expect(isValidClaudeProfileId("dot.json")).toBe(false)
    expect(isValidClaudeProfileId(FOLLOW_DEFAULT_CLAUDE_PROFILE_ID)).toBe(false)
  })
})

describe("ClaudeProfileCatalog", () => {
  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  beforeEach(() => {
    api.claudeProfileList.mockResolvedValue([FOLLOW])
    api.claudeProfileUpsert.mockResolvedValue(RELAY)
    api.claudeProfileDelete.mockResolvedValue(undefined)
  })

  // Adding a profile must not stop to ask for a name or an id first — the
  // panel names it `Settings 2` and lets the user type over that.
  it("names a new profile for you and selects its tab", async () => {
    const user = userEvent.setup()
    renderCatalog()
    await addBlankProfile(user)

    expect(screen.getByRole("tab", { name: /Settings 2/ })).toHaveAttribute(
      "aria-selected",
      "true"
    )
    expect(await screen.findByLabelText("Name")).toHaveValue("Settings 2")
    expect(screen.getByLabelText("ID")).toHaveValue("settings-2")
  })

  it("rejects an illegal id and the reserved follow-default id", async () => {
    const user = userEvent.setup()
    renderCatalog()
    await addBlankProfile(user)

    const idInput = await screen.findByLabelText("ID")
    const save = screen.getByRole("button", { name: "Save" })

    fireEvent.change(idInput, { target: { value: "Bad Id" } })
    fireEvent.click(save)
    expect(await screen.findByText(/ID must match/)).toBeInTheDocument()
    expect(api.claudeProfileUpsert).not.toHaveBeenCalled()

    fireEvent.change(idInput, {
      target: { value: FOLLOW_DEFAULT_CLAUDE_PROFILE_ID },
    })
    fireEvent.click(save)
    expect(
      await screen.findByText(/follow-default.*reserved/)
    ).toBeInTheDocument()
    expect(api.claudeProfileUpsert).not.toHaveBeenCalled()
  })

  it("optimistically inserts a saved profile without waiting for another list", async () => {
    const user = userEvent.setup()
    renderCatalog()
    await addBlankProfile(user)

    fireEvent.change(await screen.findByLabelText("ID"), {
      target: { value: "api" },
    })
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "中转" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(api.claudeProfileUpsert).toHaveBeenCalled())
    expect(await screen.findByRole("tab", { name: "中转" })).toBeInTheDocument()
    expect(api.claudeProfileList).toHaveBeenCalledTimes(1)
  })

  it("offers no editing controls while Follow default is the active tab", async () => {
    renderCatalog()
    await addProfileButton()
    expect(screen.queryByLabelText("Name")).toBeNull()
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull()
  })

  it("keeps an edit buffer per tab while switching between them", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    const user = userEvent.setup()
    renderCatalog()
    await user.click(await screen.findByRole("tab", { name: "中转" }))
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "renamed" },
    })
    // The tab renames live, and carries an unsaved marker.
    expect(screen.getByRole("tab", { name: /renamed/ })).toBeInTheDocument()
    // Leave and come back: an unsaved edit is not a reason to lose typing.
    await user.click(screen.getByRole("tab", { name: "Follow default" }))
    await user.click(screen.getByRole("tab", { name: /renamed/ }))
    expect(screen.getByLabelText("Name")).toHaveValue("renamed")
  })

  // Duplicating is the common way to make "the same endpoint, other model".
  it("duplicates the active profile but never the token", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    const user = userEvent.setup()
    renderCatalog()

    await user.click(await screen.findByRole("tab", { name: "中转" }))
    await user.click(screen.getByRole("button", { name: "Add profile" }))
    await user.click(await screen.findByRole("menuitem", { name: /Duplicate/ }))

    expect(screen.getByLabelText("Base URL")).toHaveValue(
      "https://example.test/v1"
    )
    expect(screen.getByLabelText("Auth token")).toHaveValue("")
    expect(screen.getByText(/was not copied/)).toBeInTheDocument()
  })

  // The parent renders the CLI-global settings as the Follow-default tab's
  // body, so it has to be told which tab is open — including the initial one.
  it("reports the active tab to the parent", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    const onActiveProfileChange = vi.fn()
    const user = userEvent.setup()
    renderCatalog({ onActiveProfileChange })

    expect(onActiveProfileChange).toHaveBeenCalledWith("follow-default")

    await user.click(await screen.findByRole("tab", { name: "中转" }))
    expect(onActiveProfileChange).toHaveBeenLastCalledWith("api")

    await user.click(screen.getByRole("tab", { name: "Follow default" }))
    expect(onActiveProfileChange).toHaveBeenLastCalledWith("follow-default")
  })

  it("removes a deleted profile from the tab strip after confirm", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    const user = userEvent.setup()
    renderCatalog()

    await user.click(await screen.findByRole("tab", { name: "中转" }))
    await user.click(screen.getByRole("button", { name: "Delete" }))
    await user.click(screen.getByRole("button", { name: "Confirm Delete" }))

    await waitFor(() =>
      expect(api.claudeProfileDelete).toHaveBeenCalledWith("api")
    )
    await waitFor(() =>
      expect(screen.queryByRole("tab", { name: "中转" })).toBeNull()
    )
  })
})
