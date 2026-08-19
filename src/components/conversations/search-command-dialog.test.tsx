import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { SearchCommandDialog } from "./search-command-dialog"
import enMessages from "@/i18n/messages/en.json"
import type { DbConversationSummary, FolderDetail } from "@/lib/types"

const h = vi.hoisted(() => ({
  listAll: vi.fn(),
  searchContent: vi.fn(),
  listWorkspaceFiles: vi.fn(),
  openTab: vi.fn(),
  openConversations: vi.fn(),
  openSessionCenter: vi.fn(),
  openFilePreview: vi.fn(),
  revealInFileTree: vi.fn(),
  activeFolder: null as FolderDetail | null,
  activeFolderId: null as number | null,
}))

vi.mock("@/lib/api", () => ({
  listAllConversations: h.listAll,
  // Kept on the mock as a tripwire: the palette must not reach for content
  // search again, so the test asserts this stays untouched.
  searchSessionContent: h.searchContent,
  listWorkspaceFiles: h.listWorkspaceFiles,
}))

vi.mock("@/contexts/active-folder-context", () => ({
  useActiveFolder: () => ({
    activeFolder: h.activeFolder,
    activeFolderId: h.activeFolderId,
  }),
}))

vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => ({ openTab: h.openTab }),
}))

vi.mock("@/contexts/workbench-route-context", () => ({
  useWorkbenchRoute: () => ({ openConversations: h.openConversations }),
}))

vi.mock("@/contexts/workspace-context", () => ({
  useWorkspaceActions: () => ({ openFilePreview: h.openFilePreview }),
}))

vi.mock("@/contexts/aux-panel-context", () => ({
  useAuxPanelContext: () => ({ revealInFileTree: h.revealInFileTree }),
}))

vi.mock("@/contexts/session-center-context", () => ({
  useSessionCenter: () => ({
    openSessionCenter: h.openSessionCenter,
    closedRevision: 0,
  }),
}))

function conversation(
  over: Partial<DbConversationSummary> & { id: number }
): DbConversationSummary {
  return {
    folder_id: 1,
    title: `conversation ${over.id}`,
    title_locked: false,
    agent_type: "claude_code",
    status: "in_progress",
    kind: "regular",
    model: null,
    git_branch: "main",
    external_id: null,
    message_count: 3,
    child_count: 0,
    created_at: "2026-06-10T10:00:00.000Z",
    updated_at: "2026-06-10T10:00:00.000Z",
    pinned_at: null,
    ...over,
  }
}

function renderPalette() {
  const onOpenChange = vi.fn()
  render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <SearchCommandDialog open onOpenChange={onOpenChange} />
    </NextIntlClientProvider>
  )
  return { user: userEvent.setup(), onOpenChange }
}

function queryInput(): HTMLElement {
  return screen.getByPlaceholderText("Search conversations...")
}

/** The handover row, whose label carries the current query. */
function sessionCenterRow(): HTMLElement {
  return screen.getByRole("button", { name: /Session Center/ })
}

/** A result row — cmdk binds the select handler to the item, not the label. */
async function findResultRow(title: string): Promise<HTMLElement> {
  const label = await screen.findByText(title)
  const row = label.closest("[data-slot=command-item]")
  if (!row) throw new Error(`no command row for "${title}"`)
  return row as HTMLElement
}

describe("SearchCommandDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.activeFolder = null
    h.activeFolderId = null
    h.listAll.mockResolvedValue([])
    h.searchContent.mockResolvedValue({ available: true, results: [] })
  })

  it("opens the Session matched by title and closes the palette", async () => {
    h.listAll.mockResolvedValue([
      conversation({ id: 7, folder_id: 4, title: "alpha rollout" }),
    ])
    const { user, onOpenChange } = renderPalette()

    await user.type(queryInput(), "alpha")
    const row = await findResultRow("alpha rollout")

    await user.click(row)

    expect(h.openTab).toHaveBeenCalledWith(4, 7, "claude_code", true)
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("never runs the content search", async () => {
    h.listAll.mockResolvedValue([conversation({ id: 7, title: "alpha" })])
    const { user } = renderPalette()

    await user.type(queryInput(), "alpha")
    await screen.findByText("alpha")

    // Content search is the Session Center's job now — the palette stays on
    // titles, so the ctx-backed endpoint is never reached from here.
    expect(h.searchContent).not.toHaveBeenCalled()
  })

  it("drops the agent filter chips", async () => {
    h.listAll.mockResolvedValue([
      conversation({ id: 1, title: "one" }),
      conversation({ id: 2, title: "two", agent_type: "codex" }),
    ])
    const { user } = renderPalette()

    await user.type(queryInput(), "o")
    await screen.findByText("one")

    // The chip row led with an "All" button; the scope row's "All sessions"
    // is a different control and stays.
    expect(screen.queryByRole("button", { name: "All" })).toBeNull()
    expect(screen.getByRole("button", { name: "All sessions" })).toBeTruthy()
  })

  it("hands the query to the Session Center", async () => {
    const { user, onOpenChange } = renderPalette()

    await user.type(queryInput(), "auth")
    await waitFor(() =>
      expect(sessionCenterRow().textContent).toContain("auth")
    )

    await user.click(sessionCenterRow())

    expect(h.openSessionCenter).toHaveBeenCalledWith({ search: "auth" })
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("offers the Session Center with nothing typed yet", async () => {
    const { user } = renderPalette()

    await user.click(
      screen.getByRole("button", { name: "Open Session Center" })
    )

    expect(h.openSessionCenter).toHaveBeenCalledWith({ search: "" })
  })
})
