import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { CreateRoomDialog } from "./create-room-dialog"
import enMessages from "@/i18n/messages/en.json"
import type { DbConversationSummary } from "@/lib/types"

const h = vi.hoisted(() => ({
  createCollaborationRoom: vi.fn(),
  openRoom: vi.fn(),
  tabState: {
    activeWorkbenchId: 1,
    activeTabId: "conv-1" as string | null,
    rawTabs: [] as Array<{
      id: string
      kind: "conversation" | "room"
      conversationId: number | null
      folderId: number
      agentType: string
      title: string
      isPinned: boolean
    }>,
  },
  conversations: [] as DbConversationSummary[],
}))

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock("@/lib/api", () => ({
  createCollaborationRoom: h.createCollaborationRoom,
}))

vi.mock("@/lib/open-room", () => ({
  useOpenRoom: () => h.openRoom,
}))

vi.mock("@/stores/tab-store", () => ({
  useTabStore: (selector: (state: typeof h.tabState) => unknown) =>
    selector(h.tabState),
}))

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (
    selector: (state: {
      conversations: DbConversationSummary[]
      folders: Array<{ id: number; name: string; alias: string | null }>
    }) => unknown
  ) =>
    selector({
      conversations: h.conversations,
      folders: [{ id: 7, name: "codeg", alias: null }],
    }),
}))

function conversation(
  id: number,
  title: string | null,
  archived = false
): DbConversationSummary {
  return {
    id,
    folder_id: 7,
    title,
    title_locked: true,
    agent_type: "codex",
    status: "in_progress",
    kind: "regular",
    model: null,
    git_branch: null,
    external_id: null,
    message_count: 1,
    child_count: 0,
    created_at: "2026-06-01T00:00:00.000Z",
    updated_at: "2026-06-01T00:00:00.000Z",
    archived_at: archived ? "2026-06-02T00:00:00.000Z" : null,
    pinned_at: null,
  }
}

function renderDialog() {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <CreateRoomDialog open onOpenChange={vi.fn()} />
    </NextIntlClientProvider>
  )
}

describe("CreateRoomDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.conversations.splice(
      0,
      h.conversations.length,
      conversation(1, "Alpha"),
      conversation(2, "Beta")
    )
    h.tabState.activeWorkbenchId = 1
    h.tabState.activeTabId = "conv-1"
    h.tabState.rawTabs = [
      {
        id: "conv-1",
        kind: "conversation",
        conversationId: 1,
        folderId: 7,
        agentType: "codex",
        title: "Alpha",
        isPinned: false,
      },
    ]
    h.createCollaborationRoom.mockResolvedValue({
      id: "rm_new",
      title: "Alpha's room",
    })
    h.openRoom.mockResolvedValue(undefined)
  })

  it("preselects the active Session and creates with that initiator", async () => {
    const user = userEvent.setup()
    renderDialog()

    expect(screen.getByRole("radio", { name: /Alpha/ })).toBeChecked()
    expect(screen.getByRole("radio", { name: /Beta/ })).not.toBeChecked()
    expect(screen.getByLabelText("Room title")).toHaveValue("Alpha's room")
    expect(
      screen.getByText(
        "Pick one initiator Session. Invite others after the room opens."
      )
    ).toBeTruthy()

    const create = screen.getByRole("button", { name: "Create room" })
    expect(create).toBeEnabled()
    await user.click(create)

    await waitFor(() => {
      expect(h.createCollaborationRoom).toHaveBeenCalledWith({
        workbenchId: 1,
        title: "Alpha's room",
        memberConversationIds: [1],
        createdByConversationId: 1,
      })
    })
    expect(h.openRoom).toHaveBeenCalled()
  })

  it("switches initiator with radio selection", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.click(screen.getByRole("radio", { name: /Beta/ }))
    expect(screen.getByRole("radio", { name: /Beta/ })).toBeChecked()
    expect(screen.getByRole("radio", { name: /Alpha/ })).not.toBeChecked()
    expect(screen.getByLabelText("Room title")).toHaveValue("Beta's room")
  })

  it("does not preselect when the active tab is not a Session", () => {
    h.tabState.activeTabId = "room-1"
    h.tabState.rawTabs = [
      {
        id: "room-1",
        kind: "room",
        conversationId: null,
        folderId: 7,
        agentType: "codex",
        title: "Plan",
        isPinned: false,
      },
    ]
    renderDialog()

    expect(screen.getByRole("radio", { name: /Alpha/ })).not.toBeChecked()
    expect(screen.getByRole("radio", { name: /Beta/ })).not.toBeChecked()
    expect(screen.getByRole("button", { name: "Create room" })).toBeDisabled()
  })
})
