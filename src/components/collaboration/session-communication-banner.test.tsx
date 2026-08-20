import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { CollaborationDelivery, CollaborationFeed } from "@/lib/types"

const hook = vi.hoisted(() => ({
  markSeen: vi.fn(),
  resolve: vi.fn(),
  dismiss: vi.fn(),
  restore: vi.fn(),
  retry: vi.fn(),
  feed: null as unknown,
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
vi.mock("@/hooks/use-collaboration-feed", () => ({
  useCollaborationFeed: () => ({
    feed: hook.feed,
    hydrated: true,
    error: null,
    reload: vi.fn(),
    markSeen: hook.markSeen,
    resolve: hook.resolve,
    dismiss: hook.dismiss,
    restore: hook.restore,
    retry: hook.retry,
  }),
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
import {
  SessionCommunicationBanner,
  SessionPendingContextBar,
} from "./session-communication-banner"

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

beforeEach(() => {
  vi.clearAllMocks()
  workspace.conversations = [
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
  ]
  hook.feed = {
    conversationId: 2,
    revision: 1,
    unreadCount: 1,
    inbound: [delivery()],
    outbound: [],
  }
})

describe("SessionCommunicationBanner", () => {
  it("hides the communication strip until this Session has mail", () => {
    hook.feed = {
      conversationId: 2,
      revision: 1,
      unreadCount: 0,
      inbound: [],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)

    expect(screen.queryByText("panelTitle")).not.toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "sendMenu" })
    ).not.toBeInTheDocument()
  })

  it("groups a reply chain into one thread and shows both sides of it", () => {
    hook.feed = {
      conversationId: 2,
      revision: 1,
      unreadCount: 1,
      inbound: [delivery()],
      outbound: [
        delivery({
          id: "delivery-out",
          eventId: "event-out",
          replyToEventId: "event-1",
          subject: "Re: evidence",
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
        }),
      ],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    expect(document.querySelector("[data-mailbox-panel]")).not.toBeNull()
    // The inbox lists the letter once; the reading pane shows the whole
    // exchange, our reply included.
    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(1)
    expect(
      document.querySelector("[data-thread-letter='inbound']")
    ).not.toBeNull()
    expect(
      document.querySelector("[data-thread-letter='outbound']")
    ).not.toBeNull()
    expect(screen.getAllByText("threadCount").length).toBeGreaterThan(0)
  })

  it("splits a letter into subject and body like a mail client", () => {
    hook.feed = {
      conversationId: 2,
      revision: 1,
      unreadCount: 1,
      inbound: [
        delivery({
          subject: "Proof review",
          body: "The evidence does not support the last sentence.",
        }),
      ],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    expect(screen.getAllByText("Proof review").length).toBeGreaterThan(1)
    expect(
      document.querySelector("[data-collaboration-subject]")?.textContent
    ).toBe("Proof review")
    expect(
      screen.getAllByText("The evidence does not support the last sentence.")
        .length
    ).toBeGreaterThan(0)
  })

  it("does not offer a human compose action on the Agent mailbox strip", () => {
    render(<SessionCommunicationBanner conversationId={2} />)

    expect(
      screen.queryByRole("button", { name: "sendMenu" })
    ).not.toBeInTheDocument()
  })

  it("uses the live Session title instead of the send-time snapshot", () => {
    hook.feed = {
      conversationId: 2,
      revision: 1,
      unreadCount: 1,
      inbound: [
        delivery({
          source: {
            conversationId: 1,
            title: "You are Session C. Reply with exactly C_READY.",
            agentType: "codex",
            folderPath: "/thesis",
            backend: "current",
          },
        }),
      ],
      outbound: [],
    }
    workspace.conversations = [
      {
        id: 1,
        folder_id: 10,
        title: "Session C",
        agent_type: "codex",
      },
      {
        id: 2,
        folder_id: 10,
        title: "Session D",
        agent_type: "claude_code",
      },
    ]
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    expect(screen.getByText("from:Session C")).toBeInTheDocument()
    expect(
      screen.queryByText(/You are Session C\. Reply with exactly C_READY/)
    ).not.toBeInTheDocument()
  })

  it("shows communication outside the native transcript and marks it read", () => {
    render(<SessionCommunicationBanner conversationId={2} />)
    expect(
      screen.queryByText(/evidence does not support/)
    ).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    expect(
      screen.getAllByText("The evidence does not support the last sentence.")
        .length
    ).toBeGreaterThan(0)
    expect(screen.getByText("from:Logic reviewer")).toBeInTheDocument()
    expect(screen.getAllByText("untitledSubject").length).toBeGreaterThan(0)
    expect(
      document.querySelector("[data-collaboration-banner]")
    ).toBeInTheDocument()
    expect(screen.getAllByText("mailUnread").length).toBeGreaterThan(0)
    expect(document.querySelector("[data-mail-status='unread']")).not.toBeNull()

    expect(hook.markSeen).toHaveBeenCalledWith(["delivery-1"])
  })

  it("marks newly arriving mail opened while the communication panel stays expanded", () => {
    const view = render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    hook.markSeen.mockClear()
    hook.feed = {
      conversationId: 2,
      revision: 2,
      unreadCount: 1,
      inbound: [
        delivery({
          attentionState: "opened",
          openedAt: "2026-08-16T00:01:00Z",
          uiSeenAt: "2026-08-16T00:01:00Z",
        }),
        delivery({ id: "delivery-2", eventId: "event-2" }),
      ],
      outbound: [],
    }

    view.rerender(<SessionCommunicationBanner conversationId={2} />)

    expect(hook.markSeen).toHaveBeenCalledWith(["delivery-2"])
  })

  it("opens the stable source Session without letting a human reply for the Agent", () => {
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    fireEvent.click(screen.getByRole("button", { name: "openSession" }))
    expect(tabs.openTab).toHaveBeenCalledWith(
      10,
      1,
      "codex",
      true,
      "Logic reviewer"
    )

    expect(
      screen.queryByRole("button", { name: "reply" })
    ).not.toBeInTheDocument()
  })

  it("shows whether an explicit reply obligation is still open", () => {
    hook.feed = {
      conversationId: 2,
      revision: 2,
      unreadCount: 1,
      inbound: [
        delivery({ expectsReply: true, obligationState: "awaiting_reply" }),
      ],
      outbound: [
        delivery({
          id: "delivery-outbound",
          eventId: "event-outbound",
          subject: "Status report",
          expectsReply: true,
          replyReceived: true,
          obligationState: "resolved",
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
        }),
      ],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    expect(screen.getAllByText("mailUnread").length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole("tab", { name: /sentLabel/ }))
    fireEvent.click(screen.getAllByText("Status report")[0])
    expect(screen.getAllByText("mailOutReplied").length).toBeGreaterThan(0)
    expect(screen.queryByText("mailReplied")).not.toBeInTheDocument()
  })

  it("marks an inbound request as replied and an outbound request as waiting", () => {
    hook.feed = {
      conversationId: 2,
      revision: 2,
      unreadCount: 1,
      inbound: [
        delivery({
          expectsReply: true,
          replyReceived: true,
          obligationState: "resolved",
        }),
      ],
      outbound: [
        delivery({
          id: "delivery-outbound",
          eventId: "event-outbound",
          subject: "Ping D",
          expectsReply: true,
          obligationState: "awaiting_reply",
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
        }),
      ],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    expect(screen.getAllByText("mailReplied").length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole("tab", { name: /sentLabel/ }))
    fireEvent.click(screen.getAllByText("Ping D")[0])
    expect(screen.getAllByText("mailOutUnread").length).toBeGreaterThan(0)
    expect(screen.getAllByText("stateAwaitingReply").length).toBeGreaterThan(0)
  })

  it("keeps inbox and sent as flat audit lists that open the reply chain", () => {
    hook.feed = {
      conversationId: 2,
      revision: 2,
      unreadCount: 1,
      inbound: [delivery()],
      outbound: [
        delivery({
          id: "delivery-outbound",
          eventId: "event-outbound",
          subject: "Ping D",
          body: "PING-D body",
          expectsReply: true,
          obligationState: "awaiting_reply",
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
        }),
      ],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    // The inbound thread is selected by default; the sent letter's chain
    // is not on screen yet.
    expect(document.querySelector("[data-thread-letter='outbound']")).toBeNull()

    fireEvent.click(screen.getByRole("tab", { name: /sentLabel/ }))
    const row = document.querySelector("[data-letter-row='outbound']")
    expect(row).not.toBeNull()
    // Flat rows speak the outbound vocabulary too.
    expect(row?.textContent).toContain("mailOutUnread")

    fireEvent.click(row as HTMLElement)
    expect(
      document.querySelector("[data-thread-letter='outbound']")
    ).not.toBeNull()
  })

  it("searches letters and shields the mailbox from the page context menu", () => {
    hook.feed = {
      conversationId: 2,
      revision: 1,
      unreadCount: 1,
      inbound: [
        delivery({ subject: "Proof review", body: "check claim three" }),
        delivery({
          id: "delivery-2",
          eventId: "event-2",
          subject: "Deploy done",
          body: "all green",
        }),
      ],
      outbound: [],
    }
    const ancestorContextMenu = vi.fn()
    render(
      <div onContextMenu={ancestorContextMenu}>
        <SessionCommunicationBanner conversationId={2} />
      </div>
    )
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(2)
    fireEvent.change(
      screen.getByRole("textbox", { name: "mailSearchPlaceholder" }),
      {
        target: { value: "deploy" },
      }
    )
    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(1)
    expect(screen.getAllByText("Deploy done").length).toBeGreaterThan(0)
    fireEvent.change(
      screen.getByRole("textbox", { name: "mailSearchPlaceholder" }),
      {
        target: { value: "no-such-letter" },
      }
    )
    expect(document.querySelectorAll("[data-letter-row]")).toHaveLength(0)
    expect(screen.getByText("mailFilterEmpty")).toBeInTheDocument()

    fireEvent.contextMenu(
      document.querySelector("[data-mailbox-panel]") as HTMLElement
    )
    expect(ancestorContextMenu).not.toHaveBeenCalled()
  })

  it("does not let a human waive an Agent reply obligation", () => {
    hook.feed = {
      conversationId: 2,
      revision: 2,
      unreadCount: 1,
      inbound: [
        delivery({ expectsReply: true, obligationState: "awaiting_reply" }),
      ],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    expect(
      screen.queryByRole("button", { name: "noReplyNeeded" })
    ).not.toBeInTheDocument()
    expect(hook.resolve).not.toHaveBeenCalled()
  })

  it("labels a waived obligation separately from a real reply", () => {
    hook.feed = {
      conversationId: 2,
      revision: 3,
      unreadCount: 0,
      inbound: [
        delivery({
          expectsReply: true,
          attentionState: "opened",
          obligationState: "resolved",
          replyReceived: false,
        }),
      ],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    expect(screen.getAllByText("mailUnread").length).toBeGreaterThan(0)
    expect(screen.getByText("noReplyNeeded")).toBeInTheDocument()
    expect(screen.queryByText("mailReplied")).not.toBeInTheDocument()
  })

  it("keeps a deleted or unavailable source readable without offering broken actions", () => {
    workspace.conversations = workspace.conversations.filter(
      (conversation) => conversation.id !== 1
    )
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    expect(screen.getByText("from:Logic reviewer")).toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "openSession" })
    ).not.toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "reply" })
    ).not.toBeInTheDocument()
  })

  it("dismisses a pending inbound delivery without deleting it from the feed", () => {
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    fireEvent.click(screen.getByRole("button", { name: "dismiss" }))
    expect(hook.dismiss).toHaveBeenCalledWith("delivery-1")
  })

  it("restores a dismissed delivery to a future natural turn", () => {
    hook.feed = {
      conversationId: 2,
      revision: 2,
      unreadCount: 0,
      inbound: [
        delivery({
          state: "dismissed",
          attentionState: "opened",
          openedAt: "2026-08-16T00:01:00Z",
          uiSeenAt: "2026-08-16T00:01:00Z",
        }),
      ],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    fireEvent.click(screen.getByRole("button", { name: "restore" }))
    expect(hook.restore).toHaveBeenCalledWith("delivery-1")
  })

  it("shows pending Session context beside the composer and lets the user exclude it", () => {
    const collaboration = {
      feed: hook.feed as CollaborationFeed,
      hydrated: true,
      error: null,
      reload: vi.fn(),
      markSeen: hook.markSeen,
      resolve: hook.resolve,
      dismiss: hook.dismiss,
      restore: hook.restore,
      retry: hook.retry,
    }
    render(<SessionPendingContextBar collaboration={collaboration} />)

    expect(screen.getByText("pendingContextCount")).toBeInTheDocument()
    expect(
      screen.queryByText("The evidence does not support the last sentence.")
    ).not.toBeInTheDocument()
    fireEvent.click(
      screen.getByRole("button", { name: "pendingContextReview" })
    )
    expect(
      screen.getByText("The evidence does not support the last sentence.")
    ).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "excludeFromNextTurn" }))
    expect(hook.dismiss).toHaveBeenCalledWith("delivery-1")
  })

  it("marks newly visible pending context opened without collapsing the panel", () => {
    const collaboration = (feed: CollaborationFeed) => ({
      feed,
      hydrated: true,
      error: null,
      reload: vi.fn(),
      markSeen: hook.markSeen,
      resolve: hook.resolve,
      dismiss: hook.dismiss,
      restore: hook.restore,
      retry: hook.retry,
    })
    const view = render(
      <SessionPendingContextBar
        collaboration={collaboration(hook.feed as CollaborationFeed)}
      />
    )
    fireEvent.click(
      screen.getByRole("button", { name: "pendingContextReview" })
    )
    hook.markSeen.mockClear()
    const updatedFeed: CollaborationFeed = {
      ...(hook.feed as CollaborationFeed),
      revision: 2,
      inbound: [
        delivery({
          attentionState: "opened",
          openedAt: "2026-08-16T00:01:00Z",
          uiSeenAt: "2026-08-16T00:01:00Z",
        }),
        delivery({ id: "delivery-2", eventId: "event-2" }),
      ],
    }

    view.rerender(
      <SessionPendingContextBar collaboration={collaboration(updatedFeed)} />
    )

    expect(hook.markSeen).toHaveBeenCalledWith(["delivery-2"])
  })

  it("offers an explicit retry when agent invocation failed", () => {
    hook.feed = {
      conversationId: 2,
      revision: 2,
      unreadCount: 1,
      inbound: [
        delivery({
          invocationPolicy: "invoke_when_idle",
          state: "failed",
          queueItemId: "delivery-1#2",
          queueState: "paused",
          error: "dispatch_outcome_unknown",
        }),
      ],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    fireEvent.click(screen.getByRole("button", { name: "retry" }))
    expect(hook.retry).toHaveBeenCalledWith("delivery-1#2")
  })

  it("requires confirmation before starting a delivered message in a resumed session", () => {
    hook.feed = {
      conversationId: 2,
      revision: 2,
      unreadCount: 1,
      inbound: [
        delivery({
          invocationPolicy: "invoke_when_idle",
          state: "queued",
          queueItemId: "delivery-1",
          queueState: "paused",
          queuePausedReason:
            "collaboration_target_inactive_confirmation_required",
        }),
      ],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    expect(
      screen.getByText("stateAwaitingResumeConfirmation")
    ).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "startProcessing" }))
    expect(hook.retry).toHaveBeenCalledWith("delivery-1")
  })

  it("shows a policy-frozen delivery without offering a misleading retry", () => {
    hook.feed = {
      conversationId: 2,
      revision: 3,
      unreadCount: 1,
      inbound: [
        delivery({
          invocationPolicy: "invoke_when_idle",
          state: "queued",
          queueItemId: "delivery-1",
          queueState: "paused",
          queuePausedReason: "session_collaboration_disabled",
        }),
      ],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    expect(screen.getByText("statePausedBySettings")).toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "retry" })
    ).not.toBeInTheDocument()
  })

  it("shows the durable stop operation separately from delivery state", () => {
    hook.feed = {
      conversationId: 2,
      revision: 4,
      unreadCount: 1,
      inbound: [
        delivery({
          invocationPolicy: "invoke_when_idle",
          state: "queued",
          queueItemId: "delivery-1",
          queueState: "queued",
          interruptOperationId: "interrupt-1",
          interruptState: "waiting_for_terminal",
        }),
      ],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    expect(screen.getByText("stateStoppingCurrentTask")).toBeInTheDocument()
  })
})
