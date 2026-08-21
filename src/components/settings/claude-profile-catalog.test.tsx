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

  it("rejects an illegal id and the reserved follow-default id", async () => {
    const user = userEvent.setup()
    renderCatalog()
    await screen.findByRole("button", { name: "New" })

    await user.click(screen.getByRole("button", { name: "New" }))
    const idInput = await screen.findByLabelText("ID")
    const nameInput = screen.getByLabelText("Name")
    const save = screen.getByRole("button", { name: "Save" })

    fireEvent.change(idInput, { target: { value: "Bad Id" } })
    fireEvent.change(nameInput, { target: { value: "relay" } })
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
    await screen.findByRole("button", { name: "New" })

    await user.click(screen.getByRole("button", { name: "New" }))
    fireEvent.change(await screen.findByLabelText("ID"), {
      target: { value: "api" },
    })
    fireEvent.change(screen.getByLabelText("Name"), {
      target: { value: "中转" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => expect(api.claudeProfileUpsert).toHaveBeenCalled())
    expect(await screen.findByText("中转")).toBeInTheDocument()
    expect(api.claudeProfileList).toHaveBeenCalledTimes(1)
  })

  it("does not offer edit or delete on follow-default", async () => {
    renderCatalog()
    await screen.findByRole("button", { name: "New" })
    expect(screen.queryByRole("button", { name: "Edit" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull()
  })

  it("removes a deleted profile from the list after confirm", async () => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    const user = userEvent.setup()
    renderCatalog()
    expect(await screen.findByText("中转")).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Delete" }))
    await user.click(screen.getByRole("button", { name: "Confirm Delete" }))

    await waitFor(() =>
      expect(api.claudeProfileDelete).toHaveBeenCalledWith("api")
    )
    await waitFor(() => expect(screen.queryByText("中转")).toBeNull())
  })
})
