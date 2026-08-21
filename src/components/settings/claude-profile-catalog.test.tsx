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
  claudeSettingsRead: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  claudeProfileList: (...args: unknown[]) => api.claudeProfileList(...args),
  claudeProfileUpsert: (...args: unknown[]) => api.claudeProfileUpsert(...args),
  claudeProfileDelete: (...args: unknown[]) => api.claudeProfileDelete(...args),
  claudeSettingsRead: (...args: unknown[]) => api.claudeSettingsRead(...args),
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

/** The second file-less profile: forces the official endpoint, edits nothing. */
const OFFICIAL_DIRECT: ClaudeProfileInfo = {
  id: "official-direct",
  label: "Official direct",
  kind: "officialDirect",
  authTokenMasked: "",
  isVirtual: true,
  createdAt: "1970-01-01T00:00:00Z",
  updatedAt: "1970-01-01T00:00:00Z",
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
    api.claudeSettingsRead.mockResolvedValue({
      path: "C:/Users/me/.claude/settings.json",
      text: '{\n  "env": {\n    "ANTHROPIC_API_KEY": "sk-t••••••••7890"\n  }\n}',
      exists: true,
      droppedSecretKeys: ["ANTHROPIC_API_KEY"],
    })
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

    // Same form the "Follow default" tab renders, so the connection fields are
    // labelled the way that tab has always labelled them.
    expect(screen.getByLabelText("API URL")).toHaveValue(
      "https://example.test/v1"
    )
    expect(screen.getByLabelText("API Key")).toHaveValue("")
    expect(screen.getByText(/was not copied/)).toBeInTheDocument()
  })

  // The complaint that started this: "why does the new profile look nothing
  // like Follow default — isn't it also official-vs-API, also models?" It is,
  // and both tabs now render `ClaudeConfigFields`. This pins the fields a
  // profile tab must offer so the two cannot drift apart again.
  it("offers a profile the same auth choice and model fields as the CLI tab", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    const user = userEvent.setup()
    renderCatalog()

    await user.click(await screen.findByRole("tab", { name: "中转" }))

    expect(screen.getByLabelText("Auth Mode")).toBeInTheDocument()
    expect(screen.getByLabelText("Main Model")).toBeInTheDocument()
    expect(screen.getByLabelText("Default Haiku Model")).toBeInTheDocument()
    expect(screen.getByLabelText("Default Sonnet Model")).toBeInTheDocument()
    expect(screen.getByLabelText("Default Opus Model")).toBeInTheDocument()
    expect(
      screen.getByLabelText("Reasoning Model (thinking)")
    ).toBeInTheDocument()
    expect(screen.getByLabelText("Reasoning Effort Level")).toBeInTheDocument()
    // A profile has no provider binding, so that third mode is not offered.
    expect(screen.queryByLabelText("Select Model Provider")).toBeNull()
  })

  it("treats Official direct as a read-only tab of its own", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, OFFICIAL_DIRECT, RELAY])
    const user = userEvent.setup()
    renderCatalog()

    await user.click(
      await screen.findByRole("tab", { name: "Official direct" })
    )
    expect(screen.getByText(/Forces api\.anthropic\.com/)).toBeInTheDocument()
    // No editor: it is file-less, exactly like Follow default.
    expect(screen.queryByLabelText("Name")).toBeNull()
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull()
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

  // "aren't codeg's settings just a migrated settings.json?" — yes, and this
  // is the one click that says so. One-way: nothing is written back.
  it("imports an existing settings.json into a new profile", async () => {
    const user = userEvent.setup()
    renderCatalog()

    await user.click(await addProfileButton())
    await user.click(
      await screen.findByRole("menuitem", { name: /Import from settings/ })
    )

    await waitFor(() =>
      expect(api.claudeSettingsRead).toHaveBeenCalledWith(null)
    )
    // `toHaveValue` compares whole values and does not take an asymmetric
    // matcher, so read the value and substring-check it.
    const editor =
      await screen.findByLabelText<HTMLTextAreaElement>("settings.json")
    expect(editor.value).toContain("ANTHROPIC_API_KEY")
    // The masked key cannot be saved as-is, so the editor has to say so.
    // Anchored on the sentence: the key name alone also matches the textarea.
    expect(
      screen.getByText(/came back masked.*ANTHROPIC_API_KEY/)
    ).toBeInTheDocument()
  })

  it("refuses to save a settings.json that is not a JSON object", async () => {
    const user = userEvent.setup()
    renderCatalog()
    await addBlankProfile(user)

    await user.click(
      await screen.findByRole("button", { name: "settings.json" })
    )
    fireEvent.change(screen.getByLabelText("settings.json"), {
      target: { value: "[1, 2, 3]" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    expect(
      await screen.findByText(/must be a JSON object at the top level/)
    ).toBeInTheDocument()
    expect(api.claudeProfileUpsert).not.toHaveBeenCalled()
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
