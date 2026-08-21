import {
  render,
  screen,
  waitFor,
  within,
  cleanup,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import type { ComponentProps } from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { InlineClaudeProfileSelector } from "./claude-profile-selector"
import enMessages from "@/i18n/messages/en.json"
import type { ClaudeProfileInfo } from "@/lib/types"

const api = vi.hoisted(() => ({
  claudeProfileList: vi.fn(),
  conversationSetClaudeProfile: vi.fn(),
  conversationGetClaudeProfile: vi.fn(),
  openSettingsWindow: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  claudeProfileList: (...args: unknown[]) => api.claudeProfileList(...args),
  conversationSetClaudeProfile: (...args: unknown[]) =>
    api.conversationSetClaudeProfile(...args),
  conversationGetClaudeProfile: (...args: unknown[]) =>
    api.conversationGetClaudeProfile(...args),
  openSettingsWindow: (...args: unknown[]) => api.openSettingsWindow(...args),
}))

// The chip restarts the session it belongs to, which means reading the live
// connection. Mounting the real provider here would drag the whole ACP store
// into a test about a dropdown, so the connection is stubbed and each test
// says what state it wants.
const conn = vi.hoisted(() => ({
  status: "idle" as string,
  reapplyConfig: vi.fn(),
}))

vi.mock("@/hooks/use-connection", () => ({
  useConnection: () => conn,
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

function renderSelector(
  props?: Partial<ComponentProps<typeof InlineClaudeProfileSelector>>
) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <InlineClaudeProfileSelector conversationId={12} {...props} />
    </NextIntlClientProvider>
  )
}

describe("InlineClaudeProfileSelector", () => {
  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  beforeEach(() => {
    api.claudeProfileList.mockResolvedValue([FOLLOW, RELAY])
    api.conversationSetClaudeProfile.mockResolvedValue({
      conversationId: 12,
      profileId: "api",
      affectedRunningSessions: 0,
    })
    api.openSettingsWindow.mockResolvedValue(undefined)
    api.conversationGetClaudeProfile.mockResolvedValue({
      conversationId: 12,
      profileId: "follow-default",
      affectedRunningSessions: 0,
    })
    conn.status = "idle"
    conn.reapplyConfig.mockReset()
    conn.reapplyConfig.mockResolvedValue(true)
  })

  it("keeps the chip bare and names the control inside the dropdown", async () => {
    const user = userEvent.setup()
    renderSelector()

    const trigger = await screen.findByRole("button", {
      name: "Launch profile: Follow default",
    })
    expect(trigger).toHaveTextContent("Follow default")
    expect(trigger.textContent).not.toContain(": ")

    await user.click(trigger)
    const menu = await screen.findByRole("menu")
    const labeled = within(menu).getAllByText("Launch profile")
    expect(labeled[0]).toBeInTheDocument()
  })

  it("calls conversationSetClaudeProfile when a profile is chosen", async () => {
    const user = userEvent.setup()
    renderSelector()

    await user.click(
      await screen.findByRole("button", {
        name: "Launch profile: Follow default",
      })
    )
    await user.click(await screen.findByRole("menuitemradio", { name: /中转/ }))

    await waitFor(() =>
      expect(api.conversationSetClaudeProfile).toHaveBeenCalledWith(12, "api")
    )
    expect(
      screen.getByRole("button", { name: "Launch profile: 中转" })
    ).toHaveTextContent("中转")
    expect(
      screen.getByRole("button", { name: "Launch profile: 中转" }).textContent
    ).not.toContain(": ")
  })

  it("disables the chip while a turn is running", async () => {
    renderSelector({ disabled: true })
    const trigger = await screen.findByRole("button", {
      name: "Launch profile: Follow default",
    })
    expect(trigger).toBeDisabled()
  })

  // It used to read `follow-default` on every mount, so reopening the app made
  // a session bound to a gateway claim it was following the CLI.
  it("shows the profile the conversation is actually bound to", async () => {
    api.conversationGetClaudeProfile.mockResolvedValue({
      conversationId: 12,
      profileId: "api",
      affectedRunningSessions: 0,
    })
    renderSelector()

    expect(
      await screen.findByRole("button", { name: "Launch profile: 中转" })
    ).toBeInTheDocument()
  })

  it("restarts an idle session straight away, with no dialog", async () => {
    api.conversationSetClaudeProfile.mockResolvedValue({
      conversationId: 12,
      profileId: "api",
      affectedRunningSessions: 1,
    })
    const user = userEvent.setup()
    renderSelector({ tabId: "tab-1" })

    await user.click(
      await screen.findByRole("button", {
        name: "Launch profile: Follow default",
      })
    )
    await user.click(await screen.findByRole("menuitemradio", { name: /中转/ }))

    await waitFor(() => expect(conn.reapplyConfig).toHaveBeenCalledTimes(1))
    // Nothing is lost restarting an idle session, so nothing is asked.
    expect(screen.queryByRole("alertdialog")).toBeNull()
  })

  it("asks before killing a turn in flight, and only restarts on confirm", async () => {
    conn.status = "prompting"
    api.conversationSetClaudeProfile.mockResolvedValue({
      conversationId: 12,
      profileId: "api",
      affectedRunningSessions: 1,
    })
    const user = userEvent.setup()
    renderSelector({ tabId: "tab-1" })

    await user.click(
      await screen.findByRole("button", {
        name: "Launch profile: Follow default",
      })
    )
    await user.click(await screen.findByRole("menuitemradio", { name: /中转/ }))

    const dialog = await screen.findByRole("alertdialog")
    expect(conn.reapplyConfig).not.toHaveBeenCalled()

    await user.click(
      within(dialog).getByRole("button", { name: "Restart and apply" })
    )
    await waitFor(() => expect(conn.reapplyConfig).toHaveBeenCalledTimes(1))
  })
})
