import {
  act,
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
      <InlineClaudeProfileSelector
        conversationId={12}
        agentDefaultProfileId={null}
        {...props}
      />
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
    const onPendingProfileChange = vi.fn().mockResolvedValue(true)
    const user = userEvent.setup()
    renderSelector({ onPendingProfileChange })

    await user.click(
      await screen.findByRole("button", {
        name: "Launch profile: Follow default",
      })
    )
    await user.click(await screen.findByRole("menuitemradio", { name: /中转/ }))

    await waitFor(() =>
      expect(api.conversationSetClaudeProfile).toHaveBeenCalledWith(12, "api")
    )
    expect(onPendingProfileChange).not.toHaveBeenCalled()
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

  it("lets the user pick a profile before a conversation exists", async () => {
    const onPendingProfileChange = vi.fn().mockResolvedValue(true)
    const user = userEvent.setup()
    renderSelector({
      conversationId: null,
      onPendingProfileChange,
    })

    const trigger = await screen.findByRole("button", {
      name: "Launch profile: Follow default",
    })
    expect(trigger).toBeEnabled()

    await user.click(trigger)
    await user.click(await screen.findByRole("menuitemradio", { name: /中转/ }))

    expect(api.conversationSetClaudeProfile).not.toHaveBeenCalled()
    expect(api.conversationGetClaudeProfile).not.toHaveBeenCalled()
    expect(onPendingProfileChange).toHaveBeenCalledWith("api")
    expect(
      screen.getByRole("button", { name: "Launch profile: 中转" })
    ).toHaveTextContent("中转")
    expect(
      screen.getByRole("button", { name: "Launch profile: 中转" }).textContent
    ).not.toContain(": ")

    await user.click(
      screen.getByRole("button", { name: "Launch profile: 中转" })
    )
    await user.click(
      await screen.findByRole("menuitemradio", { name: /Follow default/ })
    )
    await waitFor(() =>
      expect(onPendingProfileChange).toHaveBeenNthCalledWith(
        2,
        "follow-default"
      )
    )
    expect(
      screen.getByRole("button", { name: "Launch profile: Follow default" })
    ).toBeInTheDocument()
  })

  it("shows a pending choice the same way a saved binding is shown", async () => {
    renderSelector({ conversationId: null, pendingProfileId: "api" })
    expect(
      await screen.findByRole("button", { name: "Launch profile: 中转" })
    ).toHaveTextContent("中转")
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

  it("leaves a live session restart to the stale-config owner", async () => {
    api.conversationSetClaudeProfile.mockResolvedValue({
      conversationId: 12,
      profileId: "api",
      affectedRunningSessions: 1,
    })
    const user = userEvent.setup()
    renderSelector()

    await user.click(
      await screen.findByRole("button", {
        name: "Launch profile: Follow default",
      })
    )
    await user.click(await screen.findByRole("menuitemradio", { name: /中转/ }))

    await waitFor(() =>
      expect(api.conversationSetClaudeProfile).toHaveBeenCalledTimes(1)
    )
    expect(conn.reapplyConfig).not.toHaveBeenCalled()
    expect(screen.queryByRole("alertdialog")).toBeNull()
  })

  it("does not own the busy-session restart dialog", async () => {
    conn.status = "prompting"
    api.conversationSetClaudeProfile.mockResolvedValue({
      conversationId: 12,
      profileId: "api",
      affectedRunningSessions: 1,
    })
    const user = userEvent.setup()
    renderSelector()

    await user.click(
      await screen.findByRole("button", {
        name: "Launch profile: Follow default",
      })
    )
    await user.click(await screen.findByRole("menuitemradio", { name: /中转/ }))

    await waitFor(() =>
      expect(api.conversationSetClaudeProfile).toHaveBeenCalledTimes(1)
    )
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument()
    expect(conn.reapplyConfig).not.toHaveBeenCalled()
  })

  it("does not act on connection status after a delayed save", async () => {
    let finishSave: (value: {
      conversationId: number
      profileId: string
      affectedRunningSessions: number
    }) => void = () => {}
    api.conversationSetClaudeProfile.mockImplementation(
      () =>
        new Promise((resolve) => {
          finishSave = resolve
        })
    )
    const user = userEvent.setup()
    const view = renderSelector()

    await user.click(
      await screen.findByRole("button", {
        name: "Launch profile: Follow default",
      })
    )
    await user.click(await screen.findByRole("menuitemradio", { name: /中转/ }))
    await waitFor(() =>
      expect(api.conversationSetClaudeProfile).toHaveBeenCalledTimes(1)
    )

    conn.status = "prompting"
    view.rerender(
      <NextIntlClientProvider locale="en" messages={enMessages}>
        <InlineClaudeProfileSelector conversationId={12} />
      </NextIntlClientProvider>
    )
    await act(async () => {
      finishSave({
        conversationId: 12,
        profileId: "api",
        affectedRunningSessions: 1,
      })
    })

    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument()
    expect(conn.reapplyConfig).not.toHaveBeenCalled()
  })
})
