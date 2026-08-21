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

// Match the shared Monaco test convention: expose the controlled editor as a
// textarea so the profile's draft/save contract remains testable in jsdom.
vi.mock("@monaco-editor/react", async () => {
  const { createElement } = await import("react")
  return {
    default: ({
      value,
      onChange,
      options,
    }: {
      value?: string
      onChange?: (value: string | undefined) => void
      options?: { ariaLabel?: string }
    }) =>
      createElement("textarea", {
        "aria-label": options?.ariaLabel,
        value: value ?? "",
        onChange: (event: { target: { value: string } }) =>
          onChange?.(event.target.value),
      }),
  }
})

vi.mock("@/lib/monaco-local", () => ({}))

import { ClaudeProfileCatalog } from "./claude-profile-catalog"
import { AppearanceProvider } from "@/components/appearance-provider"
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

/** The settings.json editor's raw text. Asymmetric matchers do not work with
 *  jest-dom's `toHaveValue`, which compares strictly. */
async function editorText(): Promise<string> {
  const editor = await screen.findByLabelText("settings.json")
  return (editor as HTMLTextAreaElement).value
}

function renderCatalog(
  props?: Partial<ComponentProps<typeof ClaudeProfileCatalog>>
) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <AppearanceProvider>
        <ClaudeProfileCatalog
          defaultProfileId=""
          onSetAgentDefault={vi.fn(async () => {})}
          {...props}
        />
      </AppearanceProvider>
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

  // The whole point of showing the mask: an emptied box is an instruction the
  // backend can act on. Before this, blank meant "keep", so a gateway profile
  // switched to the official subscription kept its token and kept billing.
  describe("the stored token", () => {
    beforeEach(() => {
      api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    })

    // The token lives in a dedicated column that this profile's settings.json
    // never mentions. It is folded into the editor on open so the text really
    // is the whole profile — otherwise deriving the column back out of it on
    // save would clear the credential.
    it("is folded into the editor as its mask", async () => {
      const user = userEvent.setup()
      renderCatalog()
      await user.click(await screen.findByRole("tab", { name: "中转" }))

      const editor = await editorText()
      expect(editor).toContain(RELAY.authTokenMasked as string)
      expect(editor).toContain("https://example.test/v1")
    })

    it("is cleared when the key is removed from the editor", async () => {
      const user = userEvent.setup()
      renderCatalog()
      await user.click(await screen.findByRole("tab", { name: "中转" }))

      fireEvent.change(await screen.findByLabelText("settings.json"), {
        target: { value: JSON.stringify({ env: {} }, null, 2) },
      })
      await user.click(screen.getByRole("button", { name: "Save" }))

      await waitFor(() => expect(api.claudeProfileUpsert).toHaveBeenCalled())
      expect(api.claudeProfileUpsert.mock.calls[0][0]).toMatchObject({
        id: "api",
        authToken: "",
        baseUrl: null,
      })
    })

    // `record.env` is the third store: it overlays the raw JSON before the
    // columns do, and it has no editor either. A save that left it populated
    // would keep an invisible layer beating the text on screen.
    it("empties the env overlay so the file is the only layer left", async () => {
      const user = userEvent.setup()
      renderCatalog()
      await user.click(await screen.findByRole("tab", { name: "中转" }))

      fireEvent.change(await screen.findByLabelText("Name"), {
        target: { value: "中转 2" },
      })
      await user.click(screen.getByRole("button", { name: "Save" }))

      await waitFor(() => expect(api.claudeProfileUpsert).toHaveBeenCalled())
      expect(api.claudeProfileUpsert.mock.calls[0][0]).toMatchObject({
        env: {},
      })
    })

    // The mask travels back untouched and the backend maps it to the secret,
    // so an unrelated edit must not disturb the credential.
    it("survives an edit that never touched it", async () => {
      const user = userEvent.setup()
      renderCatalog()
      await user.click(await screen.findByRole("tab", { name: "中转" }))

      fireEvent.change(await screen.findByLabelText("Name"), {
        target: { value: "中转 2" },
      })
      await user.click(screen.getByRole("button", { name: "Save" }))

      await waitFor(() => expect(api.claudeProfileUpsert).toHaveBeenCalled())
      expect(api.claudeProfileUpsert.mock.calls[0][0]).toMatchObject({
        authToken: RELAY.authTokenMasked,
      })
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

  it("offers no editing controls while User-level settings is the active tab", async () => {
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
    await user.click(
      screen.getByRole("tab", { name: "User-level settings (~/.claude)" })
    )
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

    // The endpoint is worth copying; the secret is not. The API only hands the
    // browser a mask, so carrying it over would show bullets on a profile that
    // has no credential at all.
    const editor = await editorText()
    expect(editor).toContain("https://example.test/v1")
    expect(editor).not.toContain(RELAY.authTokenMasked as string)
    expect(editor).not.toContain("ANTHROPIC_AUTH_TOKEN")
    expect(screen.getByText(/was not copied/)).toBeInTheDocument()
  })

  // The typed form is gone on purpose. It was a second store for settings that
  // already live in the file: it could not stay in step with the JSON below it,
  // and it could not keep up with the vendor's env surface either. Anything it
  // used to offer is now written directly in that one editor.
  it("edits a profile through the file alone, with no rival fields", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    const user = userEvent.setup()
    renderCatalog()

    await user.click(await screen.findByRole("tab", { name: "中转" }))

    expect(await screen.findByLabelText("settings.json")).toBeInTheDocument()
    for (const gone of [
      "Auth Mode",
      "API URL",
      "API Key",
      "Main Model",
      "Default Haiku Model",
      "Default Sonnet Model",
      "Default Opus Model",
      "Reasoning Model (thinking)",
      "Reasoning Effort Level",
    ]) {
      expect(screen.queryByLabelText(gone)).toBeNull()
    }
  })

  it("treats Official direct as a read-only tab of its own", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, OFFICIAL_DIRECT, RELAY])
    const user = userEvent.setup()
    renderCatalog()

    await user.click(
      await screen.findByRole("tab", { name: "Official direct" })
    )
    expect(screen.getByText(/Forces api\.anthropic\.com/)).toBeInTheDocument()
    // No editor: it is file-less, exactly like User-level settings.
    expect(screen.queryByLabelText("Name")).toBeNull()
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull()
  })

  // The parent renders the CLI-global settings as the User-level settings tab's
  // body, so it has to be told which tab is open — including the initial one.
  it("reports the active tab to the parent", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    const onActiveProfileChange = vi.fn()
    const user = userEvent.setup()
    renderCatalog({ onActiveProfileChange })

    expect(onActiveProfileChange).toHaveBeenCalledWith("follow-default")

    await user.click(await screen.findByRole("tab", { name: "中转" }))
    expect(onActiveProfileChange).toHaveBeenLastCalledWith("api")

    await user.click(
      screen.getByRole("tab", { name: "User-level settings (~/.claude)" })
    )
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

    fireEvent.change(await screen.findByLabelText("settings.json"), {
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
