import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { CollaborationDelivery } from "@/lib/types"

const hook = vi.hoisted(() => ({
  markSeen: vi.fn(),
  dismiss: vi.fn(),
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
})
