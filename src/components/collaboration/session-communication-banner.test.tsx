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
const replyDialog = vi.hoisted(() => ({ props: null as unknown }))

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
vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => tabs,
}))
vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: typeof workspace) => unknown) =>
    selector(workspace),
}))
vi.mock("./session-message-composer-dialog", () => ({
  SessionMessageComposerDialog: (props: unknown) => {
    replyDialog.props = props
    return <div data-testid="reply-dialog" />
  },
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
  replyDialog.props = null
  hook.feed = {
    conversationId: 2,
    revision: 1,
    unreadCount: 1,
    inbound: [delivery()],
    outbound: [],
  }
})

describe("SessionCommunicationBanner", () => {
  it("lets the user start a Session message when this Session has no mail yet", () => {
    hook.feed = {
      conversationId: 2,
      revision: 1,
      unreadCount: 0,
      inbound: [],
      outbound: [],
    }
    render(<SessionCommunicationBanner conversationId={2} />)

    expect(screen.getByText("panelTitle")).toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "sendMenu" }))
    expect(replyDialog.props).toEqual(
      expect.objectContaining({
        sourceConversationId: 2,
        initialTargetConversationId: null,
        replyToEventId: null,
        open: true,
      })
    )
  })

  it("opens a new Session message without expanding the existing mail list", () => {
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: "sendMenu" }))

    expect(
      screen.queryByText("The evidence does not support the last sentence.")
    ).not.toBeInTheDocument()
    expect(replyDialog.props).toEqual(
      expect.objectContaining({
        sourceConversationId: 2,
        initialTargetConversationId: null,
        replyToEventId: null,
        open: true,
      })
    )
  })

  it("shows communication outside the native transcript and marks it read", () => {
    render(<SessionCommunicationBanner conversationId={2} />)
    expect(
      screen.queryByText(/evidence does not support/)
    ).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    expect(
      screen.getByText("The evidence does not support the last sentence.")
    ).toBeInTheDocument()
    expect(screen.getByText("from:Logic reviewer")).toBeInTheDocument()
    expect(screen.getByText("stateStoreOnly")).toBeInTheDocument()

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

  it("opens the stable source Session and starts a reply linked to the event", () => {
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

    fireEvent.click(screen.getByRole("button", { name: "reply" }))
    expect(screen.getByTestId("reply-dialog")).toBeInTheDocument()
    expect(replyDialog.props).toEqual(
      expect.objectContaining({
        sourceConversationId: 2,
        initialTargetConversationId: 1,
        replyToEventId: "event-1",
        open: true,
      })
    )
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
          expectsReply: true,
          replyReceived: true,
          obligationState: "resolved",
        }),
      ],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    expect(screen.getByText("stateNeedsReply")).toBeInTheDocument()
    expect(screen.getByText("stateReplyReceived")).toBeInTheDocument()
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
          expectsReply: true,
          obligationState: "awaiting_reply",
        }),
      ],
    }
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))

    expect(screen.getByText("stateReplied")).toBeInTheDocument()
    expect(screen.getByText("stateAwaitingReply")).toBeInTheDocument()
  })

  it("clears only the selected inbound obligation when no reply is needed", () => {
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
    fireEvent.click(screen.getByRole("button", { name: "noReplyNeeded" }))
    expect(hook.resolve).toHaveBeenCalledWith("delivery-1")
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

    expect(screen.getByText("noReplyNeeded")).toBeInTheDocument()
    expect(screen.queryByText("stateReplied")).not.toBeInTheDocument()
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
    fireEvent.click(screen.getByRole("button", { name: "retry" }))
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
