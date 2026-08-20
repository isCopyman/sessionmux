import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { CollaborationDelivery, CollaborationFeed } from "@/lib/types"

const hook = vi.hoisted(() => ({
  markSeen: vi.fn(),
  resolve: vi.fn(),
  dismiss: vi.fn(),
  restore: vi.fn(),
  retry: vi.fn(),
}))
const tabs = vi.hoisted(() => ({ openTab: vi.fn() }))
const workspace = vi.hoisted(() => ({
  conversations: [
    {
      id: 1,
      folder_id: 10,
      title: "Logic reviewer",
      agent_type: "codex",
    },
    {
      id: 2,
      folder_id: 10,
      title: "Writer",
      agent_type: "claude_code",
    },
  ],
}))

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) =>
    values?.name ? `${key}:${values.name}` : key,
}))
// SessionRoomChip (now inside SessionMailCard) pulls open-room -> tab-store,
// whose module init reads the real app-workspace store; this suite stubs that
// store, so stub the chip out of the import graph.
vi.mock("@/components/message/session-room-chip", () => ({
  SessionRoomChip: () => null,
}))
vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => tabs,
}))
vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: typeof workspace) => unknown) =>
    selector(workspace),
}))

import { SessionMailboxDialog } from "./session-mailbox-dialog"

function delivery(
  overrides: Partial<CollaborationDelivery> = {}
): CollaborationDelivery {
  return {
    id: "delivery-1",
    eventId: "event-1",
    source: {
      conversationId: 1,
      title: "Logic reviewer",
      agentType: "codex",
      folderPath: "/thesis",
      backend: "current",
    },
    target: {
      conversationId: 2,
      title: "Writer",
      agentType: "claude_code",
      folderPath: "/thesis",
      backend: "current",
    },
    body: "The evidence does not support the last sentence.",
    replyToEventId: null,
    expectsReply: false,
    replyReceived: false,
    urgency: "normal",
    invocationPolicy: "store_only",
    deliveryHint: "default",
    state: "pending",
    queueItemId: null,
    queueState: null,
    queuePausedReason: null,
    attentionState: "unread",
    openedAt: null,
    agentReceivedAt: null,
    agentReceiptKind: null,
    agentReceiptRef: null,
    obligationState: "none",
    obligationCreatedAt: null,
    obligationResolvedAt: null,
    uiSeenAt: null,
    embeddedTurnRef: null,
    attempts: 0,
    error: null,
    createdAt: "2026-08-16T00:00:00Z",
    updatedAt: "2026-08-16T00:00:00Z",
    ...overrides,
  }
}

function outbound(overrides: Partial<CollaborationDelivery> = {}) {
  return delivery({
    id: "delivery-out",
    eventId: "event-out",
    source: {
      conversationId: 2,
      title: "Writer",
      agentType: "claude_code",
      folderPath: "/thesis",
      backend: "current",
    },
    target: {
      conversationId: 1,
      title: "Logic reviewer",
      agentType: "codex",
      folderPath: "/thesis",
      backend: "current",
    },
    ...overrides,
  })
}

function collaboration(feed: CollaborationFeed) {
  return {
    feed,
    hydrated: true,
    error: null,
    reload: vi.fn(),
    markSeen: hook.markSeen,
    resolve: hook.resolve,
    dismiss: hook.dismiss,
    restore: hook.restore,
    retry: hook.retry,
  }
}

function renderMailbox(feed: Partial<CollaborationFeed> = {}) {
  const full: CollaborationFeed = {
    conversationId: 2,
    revision: 1,
    unreadCount: 0,
    inbound: [],
    outbound: [],
    ...feed,
  }
  render(
    <SessionMailboxDialog
      open
      onOpenChange={vi.fn()}
      conversationId={2}
      collaboration={collaboration(full)}
    />
  )
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe("SessionMailboxDialog", () => {
  it("lists inbox letters and opens the clicked letter's reply chain", () => {
    renderMailbox({
      unreadCount: 2,
      inbound: [
        delivery({ subject: "Proof review" }),
        delivery({
          id: "delivery-2",
          eventId: "event-2",
          subject: "Deploy done",
          body: "all green",
          createdAt: "2026-08-16T01:00:00Z",
        }),
      ],
    })

    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(2)
    // The newest letter's chain is the default reading pane.
    expect(screen.getAllByText("Deploy done").length).toBeGreaterThan(0)

    // Rows are newest first, so the older letter is the second row.
    const rows = document.querySelectorAll("[data-letter-row]")
    fireEvent.click(rows[1] as HTMLElement)
    // The reading pane now shows the older letter's chain; both the subject
    // and its full body land there.
    expect(
      document.querySelector("[data-thread-letter='inbound']")
    ).not.toBeNull()
    expect(
      screen.getAllByText("The evidence does not support the last sentence.")
        .length
    ).toBeGreaterThan(0)
  })

  it("marks unread inbound letters seen when the mailbox opens", () => {
    renderMailbox({
      unreadCount: 1,
      inbound: [delivery()],
    })
    expect(hook.markSeen).toHaveBeenCalledWith(["delivery-1"])
  })

  it("switches to the sent folder with outbound vocabulary on the rows", () => {
    renderMailbox({
      outbound: [
        outbound({
          subject: "Status report",
          expectsReply: true,
          obligationState: "awaiting_reply",
          // Received but unanswered: the read_awaiting state, whose outbound
          // chip must read "waiting on them".
          agentReceivedAt: "2026-08-16T00:05:00Z",
        }),
      ],
    })

    fireEvent.click(screen.getByRole("tab", { name: /sentLabel/ }))
    const row = document.querySelector("[data-letter-row='outbound']")
    expect(row).not.toBeNull()
    // Outbound awaiting reads "waiting on them", never the inbound wording.
    expect(row?.textContent).toContain("mailOutAwaitingReply")
    expect(row?.textContent).not.toContain("mailReadAwaitingReply")
  })

  it("filters inbox letters to the unread ones", () => {
    renderMailbox({
      unreadCount: 1,
      inbound: [
        delivery({ subject: "Unread letter" }),
        delivery({
          id: "delivery-2",
          eventId: "event-2",
          subject: "Read letter",
          attentionState: "opened",
          agentReceivedAt: "2026-08-16T00:05:00Z",
          openedAt: "2026-08-16T00:05:00Z",
          uiSeenAt: "2026-08-16T00:05:00Z",
        }),
      ],
    })

    fireEvent.click(screen.getByRole("radio", { name: "mailUnread" }))
    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(1)
    expect(screen.getAllByText("Unread letter").length).toBeGreaterThan(0)
    expect(screen.queryByText("Read letter")).toBeNull()
  })

  it("splits the reply-duty filter by folder: needs reply in, awaiting out", () => {
    renderMailbox({
      unreadCount: 1,
      inbound: [
        delivery({ expectsReply: true, obligationState: "awaiting_reply" }),
        delivery({ id: "delivery-2", eventId: "event-2", subject: "FYI" }),
      ],
      outbound: [
        outbound({ expectsReply: true, obligationState: "awaiting_reply" }),
        outbound({
          id: "delivery-out-2",
          eventId: "event-out-2",
          subject: "No answer needed",
        }),
      ],
    })

    // Inbox: needs_reply keeps only the letter that owes a reply.
    fireEvent.click(screen.getByRole("radio", { name: "stateNeedsReply" }))
    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(1)

    // Sent: the same concept reads as awaiting_reply and keeps only the
    // outbound letter still waiting on the peer.
    fireEvent.click(screen.getByRole("tab", { name: /sentLabel/ }))
    fireEvent.click(screen.getByRole("radio", { name: "stateAwaitingReply" }))
    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(1)
    expect(screen.queryByText("No answer needed")).toBeNull()
  })

  it("searches letters by subject, body and peer name", () => {
    renderMailbox({
      inbound: [
        delivery({ subject: "Proof review", body: "check claim three" }),
        delivery({
          id: "delivery-2",
          eventId: "event-2",
          subject: "Deploy done",
          body: "all green",
        }),
      ],
    })

    fireEvent.change(
      screen.getByRole("textbox", { name: "mailSearchPlaceholder" }),
      { target: { value: "claim three" } }
    )
    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(1)

    fireEvent.change(
      screen.getByRole("textbox", { name: "mailSearchPlaceholder" }),
      { target: { value: "nothing matches this" } }
    )
    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(0)
    expect(screen.getByText("mailFilterEmpty")).toBeInTheDocument()
  })

  it("dismisses a pending letter and restores a dismissed one", () => {
    renderMailbox({
      inbound: [
        delivery(),
        // Same reply chain, so one thread puts both cards in the reading
        // pane: the pending letter gets dismiss, the dismissed one restore.
        delivery({
          id: "delivery-2",
          eventId: "event-2",
          replyToEventId: "event-1",
          subject: "Dismissed letter",
          state: "dismissed",
          attentionState: "opened",
          agentReceivedAt: "2026-08-16T00:05:00Z",
          openedAt: "2026-08-16T00:05:00Z",
          uiSeenAt: "2026-08-16T00:05:00Z",
          createdAt: "2026-08-16T01:00:00Z",
        }),
      ],
    })

    fireEvent.click(screen.getByRole("button", { name: "dismiss" }))
    expect(hook.dismiss).toHaveBeenCalledWith("delivery-1")

    fireEvent.click(screen.getByRole("button", { name: "restore" }))
    expect(hook.restore).toHaveBeenCalledWith("delivery-2")
  })

  it("offers retry only for a failed wake, then retries its queue item", () => {
    renderMailbox({
      inbound: [
        delivery({
          invocationPolicy: "invoke_when_idle",
          state: "failed",
          queueItemId: "delivery-1#2",
          error: "dispatch_outcome_unknown",
        }),
      ],
    })

    fireEvent.click(screen.getByRole("button", { name: "retry" }))
    expect(hook.retry).toHaveBeenCalledWith("delivery-1#2")
  })

  it("opens the peer Session from a letter card", () => {
    renderMailbox({
      inbound: [delivery()],
    })

    fireEvent.click(screen.getAllByRole("button", { name: "openSession" })[0])
    expect(tabs.openTab).toHaveBeenCalledWith(
      10,
      1,
      "codex",
      true,
      "Logic reviewer"
    )
  })

  it("shows the empty-folder hint when a filter leaves nothing", () => {
    renderMailbox({ inbound: [] })
    expect(screen.getByText("inboxEmpty")).toBeInTheDocument()
  })
})
