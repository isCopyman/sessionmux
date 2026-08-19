import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type {
  CollaborationChanged,
  CollaborationUnreadOverview,
} from "@/lib/types"

const api = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock("@/lib/api", () => ({
  getCollaborationUnreadOverview: api.get,
}))

const transport = vi.hoisted(() => ({
  handlers: new Set<(change: CollaborationChanged) => void>(),
  reconnect: null as (() => void) | null,
}))
vi.mock("@/lib/platform", () => ({
  subscribe: vi.fn(
    async (_event: string, handler: (change: CollaborationChanged) => void) => {
      transport.handlers.add(handler)
      return () => transport.handlers.delete(handler)
    }
  ),
  onTransportReconnect: vi.fn((callback: () => void) => {
    transport.reconnect = callback
    return () => {
      if (transport.reconnect === callback) transport.reconnect = null
    }
  }),
}))

import { useCollaborationUnreadOverview } from "./use-collaboration-unread-overview"

function overview(
  entries: Array<[conversationId: number, unreadCount: number]>
): CollaborationUnreadOverview {
  return {
    totalUnreadCount: entries.reduce((total, [, count]) => total + count, 0),
    totalNeedsReplyCount: 0,
    totalAwaitingReplyCount: 0,
    totalFailedCount: 0,
    totalRoomUnreadCount: 0,
    totalRoomNeedsReplyCount: 0,
    sessions: entries.map(([conversationId, unreadCount]) => ({
      conversationId,
      revision: 1,
      unreadCount,
      needsReplyCount: 0,
      awaitingReplyCount: 0,
      failedCount: 0,
    })),
  }
}

function emit(conversationIds: number[]) {
  for (const handler of transport.handlers) handler({ conversationIds })
}

beforeEach(() => {
  vi.clearAllMocks()
  transport.handlers.clear()
  transport.reconnect = null
  api.get.mockResolvedValue(overview([]))
})

describe("useCollaborationUnreadOverview", () => {
  it("subscribes before hydrating and refetches the authoritative projection", async () => {
    api.get.mockResolvedValueOnce(overview([[7, 2]])).mockResolvedValueOnce(
      overview([
        [7, 1],
        [8, 3],
      ])
    )
    const { result } = renderHook(() => useCollaborationUnreadOverview())
    await waitFor(() => expect(result.current.hydrated).toBe(true))
    expect(result.current.unreadByConversation.get(7)).toBe(2)

    act(() => emit([7, 8]))
    await waitFor(() =>
      expect(result.current.overview.totalUnreadCount).toBe(4)
    )
    expect(result.current.unreadByConversation.get(7)).toBe(1)
    expect(result.current.unreadByConversation.get(8)).toBe(3)
  })

  it("keeps two windows in sync through backend events", async () => {
    api.get.mockResolvedValue(overview([[7, 1]]))
    const first = renderHook(() => useCollaborationUnreadOverview())
    const second = renderHook(() => useCollaborationUnreadOverview())
    await waitFor(() => {
      expect(first.result.current.unreadByConversation.get(7)).toBe(1)
      expect(second.result.current.unreadByConversation.get(7)).toBe(1)
      expect(transport.handlers.size).toBe(2)
    })

    api.get.mockResolvedValue(overview([]))
    act(() => emit([1, 7]))
    await waitFor(() => {
      expect(first.result.current.unreadByConversation.has(7)).toBe(false)
      expect(second.result.current.unreadByConversation.has(7)).toBe(false)
    })
  })

  it("ignores a slower snapshot after a newer reload has completed", async () => {
    let resolveOld!: (value: CollaborationUnreadOverview) => void
    api.get
      .mockImplementationOnce(
        () =>
          new Promise<CollaborationUnreadOverview>((resolve) => {
            resolveOld = resolve
          })
      )
      .mockResolvedValueOnce(overview([[8, 4]]))
    const { result } = renderHook(() => useCollaborationUnreadOverview())
    await waitFor(() => expect(transport.handlers.size).toBe(1))

    act(() => emit([8]))
    await waitFor(() =>
      expect(result.current.unreadByConversation.get(8)).toBe(4)
    )
    await act(async () => resolveOld(overview([[7, 9]])))
    expect(result.current.unreadByConversation.has(7)).toBe(false)
  })

  it("reloads after a transport reconnect", async () => {
    api.get
      .mockResolvedValueOnce(overview([]))
      .mockResolvedValueOnce(overview([[5, 1]]))
    const { result } = renderHook(() => useCollaborationUnreadOverview())
    await waitFor(() => expect(result.current.hydrated).toBe(true))

    act(() => transport.reconnect?.())
    await waitFor(() =>
      expect(result.current.unreadByConversation.get(5)).toBe(1)
    )
  })
})
