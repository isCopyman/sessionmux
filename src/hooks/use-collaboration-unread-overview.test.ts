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

/** Trailing window the hook batches incoming reload triggers into. */
const RELOAD_COALESCE_MS = 250

/** Let the subscribe → hydrate promise chain settle under fake timers. */
const settle = () =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(0)
  })

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

  it("coalesces a burst of change events into a single refetch", async () => {
    // A busy Room emits one event per message and `unread_overview` is a
    // whole-table query; back-to-back reloads are the expensive failure mode.
    vi.useFakeTimers()
    try {
      const { result } = renderHook(() => useCollaborationUnreadOverview())
      await settle()
      expect(result.current.hydrated).toBe(true)
      expect(api.get).toHaveBeenCalledTimes(1) // the initial snapshot

      act(() => {
        emit([1])
        emit([2])
        emit([3])
      })
      // Trailing-only: the first event waits out the window with the rest, so
      // nothing has been issued yet.
      expect(api.get).toHaveBeenCalledTimes(1)

      await act(async () => {
        await vi.advanceTimersByTimeAsync(RELOAD_COALESCE_MS)
      })
      expect(api.get).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it("drops a pending coalesced reload when the hook unmounts", async () => {
    vi.useFakeTimers()
    try {
      const { unmount } = renderHook(() => useCollaborationUnreadOverview())
      await settle()
      act(() => emit([1]))
      unmount()
      await act(async () => {
        await vi.advanceTimersByTimeAsync(RELOAD_COALESCE_MS)
      })
      // Only the hydration call; the queued one went out with the timer.
      expect(api.get).toHaveBeenCalledTimes(1)
    } finally {
      vi.useRealTimers()
    }
  })

  it("reloads after a transport reconnect", async () => {
    api.get
      .mockResolvedValueOnce(overview([]))
      .mockResolvedValueOnce(overview([[5, 1]]))
    const { result } = renderHook(() => useCollaborationUnreadOverview())
    await waitFor(() => expect(result.current.hydrated).toBe(true))

    // Routed through the same coalescing window as change events, so this
    // lands one window later rather than immediately.
    act(() => transport.reconnect?.())
    await waitFor(() =>
      expect(result.current.unreadByConversation.get(5)).toBe(1)
    )
  })
})
