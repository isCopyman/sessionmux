import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type {
  CollaborationChanged,
  CollaborationDelivery,
  CollaborationFeed,
} from "@/lib/types"

const api = vi.hoisted(() => ({
  get: vi.fn(),
  markSeen: vi.fn(),
  dismiss: vi.fn(),
  restore: vi.fn(),
  getQueue: vi.fn(),
  retryQueueItem: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  getCollaborationFeed: api.get,
  markCollaborationSeen: api.markSeen,
  dismissCollaborationDelivery: api.dismiss,
  restoreCollaborationDelivery: api.restore,
  getPromptQueue: api.getQueue,
  retryPromptQueueItem: api.retryQueueItem,
}))

const handlers = new Set<(change: CollaborationChanged) => void>()
vi.mock("@/lib/platform", () => ({
  subscribe: vi.fn(
    async (_event: string, handler: (change: CollaborationChanged) => void) => {
      handlers.add(handler)
      return () => handlers.delete(handler)
    }
  ),
  onTransportReconnect: vi.fn(() => () => {}),
}))

import { useCollaborationFeed } from "./use-collaboration-feed"

function delivery(id: string, seen = false): CollaborationDelivery {
  return {
    id,
    eventId: `event-${id}`,
    source: {
      conversationId: 1,
      title: "Source",
      agentType: "codex",
      folderPath: "/source",
      backend: "current",
    },
    target: {
      conversationId: 7,
      title: "Target",
      agentType: "claude_code",
      folderPath: "/target",
      backend: "current",
    },
    body: "Please review",
    replyToEventId: null,
    expectsReply: false,
    replyReceived: false,
    urgency: "normal",
    invocationPolicy: "store_only",
    deliveryHint: "default",
    state: "pending",
    uiSeenAt: seen ? "2026-08-16T00:01:00Z" : null,
    embeddedTurnRef: null,
    attempts: 0,
    error: null,
    createdAt: "2026-08-16T00:00:00Z",
    updatedAt: "2026-08-16T00:00:00Z",
  }
}

function feed(
  conversationId: number,
  revision: number,
  items: CollaborationDelivery[] = []
): CollaborationFeed {
  return {
    conversationId,
    revision,
    unreadCount: items.filter((item) => !item.uiSeenAt).length,
    inbound: items,
    outbound: [],
  }
}

function emit(conversationIds: number[]) {
  for (const handler of handlers) handler({ conversationIds })
}

beforeEach(() => {
  vi.clearAllMocks()
  handlers.clear()
  api.get.mockImplementation((id: number) => Promise.resolve(feed(id, 0)))
  api.getQueue.mockResolvedValue({
    conversationId: 7,
    revision: 4,
    pausedReason: "dispatch_outcome_unknown",
    items: [{ id: "mail", originEventId: "event-mail", state: "paused" }],
  })
  api.retryQueueItem.mockResolvedValue({
    conversationId: 7,
    revision: 5,
    pausedReason: null,
    items: [{ id: "mail", originEventId: "event-mail", state: "queued" }],
  })
})

describe("useCollaborationFeed", () => {
  it("hydrates and refetches only for an addressed Session", async () => {
    api.get
      .mockResolvedValueOnce(feed(7, 1))
      .mockResolvedValueOnce(feed(7, 2, [delivery("mail")]))
    const { result } = renderHook(() => useCollaborationFeed(7))
    await waitFor(() => expect(result.current.hydrated).toBe(true))

    act(() => emit([8]))
    expect(api.get).toHaveBeenCalledTimes(1)
    act(() => emit([7]))
    await waitFor(() => expect(result.current.feed.revision).toBe(2))
    expect(result.current.feed.unreadCount).toBe(1)
  })

  it("keeps two views on the backend revision instead of local unread state", async () => {
    api.get.mockResolvedValue(feed(7, 1, [delivery("mail")]))
    const first = renderHook(() => useCollaborationFeed(7))
    const second = renderHook(() => useCollaborationFeed(7))
    await waitFor(() => {
      expect(first.result.current.feed.unreadCount).toBe(1)
      expect(second.result.current.feed.unreadCount).toBe(1)
      expect(handlers.size).toBe(2)
    })

    api.get.mockResolvedValue(feed(7, 2, [delivery("mail", true)]))
    act(() => emit([1, 7]))
    await waitFor(() => {
      expect(first.result.current.feed.unreadCount).toBe(0)
      expect(second.result.current.feed.unreadCount).toBe(0)
    })
  })

  it("applies mark-seen, dismiss, and restore responses as authoritative snapshots", async () => {
    const mail = delivery("mail")
    api.get.mockResolvedValue(feed(7, 1, [mail]))
    api.markSeen.mockResolvedValue(feed(7, 2, [delivery("mail", true)]))
    api.dismiss.mockResolvedValue({
      ...feed(7, 3, [{ ...delivery("mail", true), state: "dismissed" }]),
      unreadCount: 0,
    })
    api.restore.mockResolvedValue({
      ...feed(7, 4, [delivery("mail", true)]),
      unreadCount: 0,
    })
    const { result } = renderHook(() => useCollaborationFeed(7))
    await waitFor(() => expect(result.current.feed.unreadCount).toBe(1))

    await act(async () => result.current.markSeen(["mail"]))
    expect(api.markSeen).toHaveBeenCalledWith(7, ["mail"])
    expect(result.current.feed.unreadCount).toBe(0)

    await act(async () => result.current.dismiss("mail"))
    expect(api.dismiss).toHaveBeenCalledWith(7, "mail")
    expect(result.current.feed.inbound[0].state).toBe("dismissed")

    await act(async () => result.current.restore("mail"))
    expect(api.restore).toHaveBeenCalledWith(7, "mail")
    expect(result.current.feed.inbound[0].state).toBe("pending")
    expect(result.current.feed.unreadCount).toBe(0)
  })

  it("does not let a slow old-Session response overwrite a new Session", async () => {
    let resolveOld!: (value: CollaborationFeed) => void
    api.get.mockImplementation((id: number) => {
      if (id === 7) {
        return new Promise<CollaborationFeed>(
          (resolve) => (resolveOld = resolve)
        )
      }
      return Promise.resolve(
        feed(8, 4, [
          {
            ...delivery("new"),
            target: { ...delivery("new").target, conversationId: 8 },
          },
        ])
      )
    })
    const { result, rerender } = renderHook(
      ({ id }: { id: number }) => useCollaborationFeed(id),
      { initialProps: { id: 7 } }
    )
    rerender({ id: 8 })
    await waitFor(() => expect(result.current.feed.conversationId).toBe(8))
    await act(async () => resolveOld(feed(7, 9, [delivery("old")])))
    expect(result.current.feed.conversationId).toBe(8)
    expect(result.current.feed.inbound[0].id).toBe("new")
  })

  it("retries a failed invocation through the authoritative Session queue", async () => {
    api.get
      .mockResolvedValueOnce(feed(7, 1, [delivery("mail")]))
      .mockResolvedValueOnce(
        feed(7, 2, [
          {
            ...delivery("mail"),
            invocationPolicy: "invoke_when_idle",
            state: "queued",
          },
        ])
      )
    const { result } = renderHook(() => useCollaborationFeed(7))
    await waitFor(() => expect(result.current.hydrated).toBe(true))

    await act(async () => result.current.retry("mail"))
    expect(api.retryQueueItem).toHaveBeenCalledWith(7, "mail", 4)
    expect(result.current.feed.inbound[0].state).toBe("queued")
  })
})
