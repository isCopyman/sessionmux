import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type {
  CollaborationChanged,
  CollaborationTimelineProjection,
} from "@/lib/types"

const api = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock("@/lib/api", () => ({
  getCollaborationTimelineProjection: api.get,
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

import {
  resolveCollaborationTimelineId,
  useCollaborationTimeline,
} from "./use-collaboration-timeline"

function projection(
  conversationId: number,
  revision: number
): CollaborationTimelineProjection {
  return { conversationId, revision, inbound: [] }
}

beforeEach(() => {
  vi.clearAllMocks()
  handlers.clear()
  api.get.mockImplementation((id: number) => Promise.resolve(projection(id, 1)))
})

describe("resolveCollaborationTimelineId", () => {
  it("prefers the bound DB id over a draft runtime key", () => {
    expect(resolveCollaborationTimelineId(-7, 288)).toBe(288)
    expect(resolveCollaborationTimelineId(288, 288)).toBe(288)
    expect(resolveCollaborationTimelineId(-7, null)).toBe(-7)
    expect(resolveCollaborationTimelineId(288, null)).toBe(288)
  })
})

describe("useCollaborationTimeline", () => {
  it("does not fetch a timeline for a draft virtual Session id", async () => {
    renderHook(() => useCollaborationTimeline(-42))
    await waitFor(() => expect(api.get).not.toHaveBeenCalled())
  })

  it("refetches every open view from the shared revision event", async () => {
    const first = renderHook(() => useCollaborationTimeline(7))
    const second = renderHook(() => useCollaborationTimeline(7))
    await waitFor(() => {
      expect(first.result.current.revision).toBe(1)
      expect(second.result.current.revision).toBe(1)
      expect(handlers.size).toBe(2)
    })

    api.get.mockResolvedValue(projection(7, 2))
    act(() => {
      for (const handler of handlers) handler({ conversationIds: [7] })
    })
    await waitFor(() => {
      expect(first.result.current.revision).toBe(2)
      expect(second.result.current.revision).toBe(2)
    })
  })

  it("ignores unrelated Session revisions and stale old-Session responses", async () => {
    let resolveOld!: (value: CollaborationTimelineProjection) => void
    api.get.mockImplementation((id: number) =>
      id === 7
        ? new Promise<CollaborationTimelineProjection>((resolve) => {
            resolveOld = resolve
          })
        : Promise.resolve(projection(id, 4))
    )
    const { result, rerender } = renderHook(
      ({ id }: { id: number }) => useCollaborationTimeline(id),
      { initialProps: { id: 7 } }
    )
    act(() => {
      for (const handler of handlers) handler({ conversationIds: [9] })
    })
    expect(api.get).toHaveBeenCalledTimes(1)

    rerender({ id: 8 })
    await waitFor(() => expect(result.current.revision).toBe(4))
    await act(async () => resolveOld(projection(7, 99)))
    expect(result.current).toEqual(projection(8, 4))
  })
})
