import { type ComponentProps, type ReactElement } from "react"
import { render, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { describe, expect, it, vi, beforeEach } from "vitest"

import enMessages from "@/i18n/messages/en.json"

// The header is a SINGLE instance reused across active tabs, and the global
// tab-switch / close-tab shortcuts still fire while a rename/delete dialog is
// open. These tests pin the regression Codex flagged: a confirm must act on the
// conversation the dialog was OPENED for, not whatever is active at confirm
// time. We open the dialog for A, rerender the same instance as B (simulating a
// mid-dialog tab switch), then confirm — and assert A is mutated, never B.
const h = vi.hoisted(() => ({
  updateConversationTitle: vi.fn(async () => {}),
  deleteConversation: vi.fn(async () => {}),
  updateConversationStatus: vi.fn(async () => {}),
  updateConversationPinned: vi.fn(async () => {}),
  closeTab: vi.fn(),
  openNewConversationTab: vi.fn(),
  updateConversationLocal: vi.fn(),
  refreshConversations: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  updateConversationTitle: h.updateConversationTitle,
  deleteConversation: h.deleteConversation,
  updateConversationStatus: h.updateConversationStatus,
  updateConversationPinned: h.updateConversationPinned,
}))
vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => ({
    closeTab: h.closeTab,
    openNewConversationTab: h.openNewConversationTab,
  }),
}))
vi.mock("@/stores/app-workspace-store", () => {
  const state = {
    updateConversationLocal: h.updateConversationLocal,
    refreshConversations: h.refreshConversations,
    conversations: [] as unknown[],
  }
  const useStore = (selector: (s: typeof state) => unknown) => selector(state)
  useStore.getState = () => state
  return { useAppWorkspaceStore: useStore }
})
vi.mock("@/stores/conversation-runtime-store", () => ({
  getRuntimeSession: () => null,
}))
vi.mock("./session-details-dialog", () => ({
  SessionDetailsDialog: () => null,
}))
vi.mock("@/components/rooms/join-room-dialog", () => ({
  JoinRoomDialog: ({
    open,
    conversationId,
  }: {
    open: boolean
    conversationId: number
  }) =>
    open ? (
      <div data-testid="join-room-dialog">join:{conversationId}</div>
    ) : null,
}))

// The header now embeds the folder picker (self-contained, store-driven); stub
// it so these tests exercise only the header's own menu/dialog logic.
vi.mock("@/components/chat/conversation-context-bar", () => ({
  ConversationHeaderFolderPicker: () => null,
}))

import { ConversationDetailHeader } from "./conversation-detail-header"

type Props = ComponentProps<typeof ConversationDetailHeader>

const A: Props = {
  tabId: "tab-a",
  conversationId: 1,
  runtimeConversationId: null,
  folderId: 1,
  folderPath: "/a",
  title: "conv-a",
  status: "in_progress",
}
const B: Props = {
  ...A,
  tabId: "tab-b",
  conversationId: 2,
  title: "conv-b",
}

function withIntl(ui: ReactElement) {
  return (
    <NextIntlClientProvider locale="en" messages={enMessages}>
      {ui}
    </NextIntlClientProvider>
  )
}

describe("ConversationDetailHeader dialog target snapshot", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("deletes the conversation the dialog was opened for, even after the active tab switches", async () => {
    // pointerEventsCheck off: Radix toggles body pointer-events while a menu is
    // open, which user-event's default guard would trip on in jsdom.
    const user = userEvent.setup({ pointerEventsCheck: 0 })
    const { rerender, getByLabelText, getByRole } = render(
      withIntl(<ConversationDetailHeader {...A} />)
    )

    await user.click(getByLabelText("More actions"))
    await user.click(getByRole("menuitem", { name: "Delete" }))

    // Simulate a mid-dialog tab switch: same header instance, now scoped to B.
    rerender(withIntl(<ConversationDetailHeader {...B} />))

    await user.click(getByRole("button", { name: "Delete" }))

    await waitFor(() => {
      expect(h.deleteConversation).toHaveBeenCalledWith(1)
      expect(h.closeTab).toHaveBeenCalledWith("tab-a")
    })
    expect(h.deleteConversation).not.toHaveBeenCalledWith(2)
    expect(h.closeTab).not.toHaveBeenCalledWith("tab-b")
  })

  it("renames the conversation the dialog was opened for, even after the active tab switches", async () => {
    const user = userEvent.setup({ pointerEventsCheck: 0 })
    const { rerender, getByLabelText, getByRole } = render(
      withIntl(<ConversationDetailHeader {...A} />)
    )

    await user.click(getByLabelText("More actions"))
    await user.click(getByRole("menuitem", { name: "Rename" }))

    rerender(withIntl(<ConversationDetailHeader {...B} />))

    const input = getByRole("textbox")
    await user.clear(input)
    await user.type(input, "renamed")
    await user.click(getByRole("button", { name: "Save" }))

    await waitFor(() => {
      expect(h.updateConversationTitle).toHaveBeenCalledWith(1, "renamed")
    })
    expect(h.updateConversationTitle).not.toHaveBeenCalledWith(2, "renamed")
  })

  it("does not offer a human Session-mail send action in the header", () => {
    const { queryByLabelText, queryByTestId } = render(
      withIntl(<ConversationDetailHeader {...A} />)
    )

    expect(queryByLabelText("Send to another session")).toBeNull()
    expect(queryByTestId("session-message-composer")).toBeNull()
  })
})

describe("ConversationDetailHeader join-room entry", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("offers Join room in the overflow menu and opens the dialog", async () => {
    const user = userEvent.setup({ pointerEventsCheck: 0 })
    const { getByLabelText, getByRole, getByTestId, queryByTestId } = render(
      withIntl(<ConversationDetailHeader {...A} />)
    )

    expect(queryByTestId("join-room-dialog")).toBeNull()
    await user.click(getByLabelText("More actions"))
    await user.click(getByRole("menuitem", { name: "Join room" }))

    expect(getByTestId("join-room-dialog")).toHaveTextContent("join:1")
  })

  it("keeps the join target when the active tab switches mid-dialog", async () => {
    const user = userEvent.setup({ pointerEventsCheck: 0 })
    const { rerender, getByLabelText, getByRole, getByTestId } = render(
      withIntl(<ConversationDetailHeader {...A} />)
    )

    await user.click(getByLabelText("More actions"))
    await user.click(getByRole("menuitem", { name: "Join room" }))

    rerender(withIntl(<ConversationDetailHeader {...B} />))

    expect(getByTestId("join-room-dialog")).toHaveTextContent("join:1")
    expect(getByTestId("join-room-dialog")).not.toHaveTextContent("join:2")
  })

  it("disables Join room until the Session is persisted", async () => {
    const user = userEvent.setup({ pointerEventsCheck: 0 })
    const draft: Props = { ...A, conversationId: null }
    const { getByLabelText, getByRole, queryByTestId } = render(
      withIntl(<ConversationDetailHeader {...draft} />)
    )

    await user.click(getByLabelText("More actions"))
    const item = getByRole("menuitem", { name: "Join room" })
    expect(item).toHaveAttribute("data-disabled")
    await user.click(item)
    expect(queryByTestId("join-room-dialog")).toBeNull()
  })
})
