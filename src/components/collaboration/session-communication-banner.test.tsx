import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { CollaborationDelivery } from "@/lib/types"

const hook = vi.hoisted(() => ({
  markSeen: vi.fn(),
  dismiss: vi.fn(),
  retry: vi.fn(),
  feed: null as unknown,
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
    dismiss: hook.dismiss,
    retry: hook.retry,
  }),
}))

import { SessionCommunicationBanner } from "./session-communication-banner"

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
    urgency: "normal",
    invocationPolicy: "store_only",
    deliveryHint: "default",
    state: "pending",
    queueItemId: null,
    queueState: null,
    queuePausedReason: null,
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
  hook.feed = {
    conversationId: 2,
    revision: 1,
    unreadCount: 1,
    inbound: [delivery()],
    outbound: [],
  }
})

describe("SessionCommunicationBanner", () => {
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

    fireEvent.click(screen.getByRole("button", { name: "markAllRead" }))
    expect(hook.markSeen).toHaveBeenCalledWith(["delivery-1"])
  })

  it("dismisses a pending inbound delivery without deleting it from the feed", () => {
    render(<SessionCommunicationBanner conversationId={2} />)
    fireEvent.click(screen.getByRole("button", { name: /panelTitle/ }))
    fireEvent.click(screen.getByRole("button", { name: "dismiss" }))
    expect(hook.dismiss).toHaveBeenCalledWith("delivery-1")
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
