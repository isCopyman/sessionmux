import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { PromptDraft, PromptQueueSnapshot } from "@/lib/types"

const api = vi.hoisted(() => ({
  get: vi.fn(),
  enqueue: vi.fn(),
  edit: vi.fn(),
  delete: vi.fn(),
  reorder: vi.fn(),
  pauseManual: vi.fn(),
  releaseOne: vi.fn(),
  resume: vi.fn(),
  retry: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  getPromptQueue: api.get,
  enqueuePromptQueueItem: api.enqueue,
  editPromptQueueItem: api.edit,
  deletePromptQueueItem: api.delete,
  reorderPromptQueueItems: api.reorder,
  pausePromptQueueForManualReview: api.pauseManual,
  releaseOnePromptQueueItem: api.releaseOne,
  resumePromptQueue: api.resume,
  retryPromptQueueItem: api.retry,
}))

const eventHandlers = new Set<(snapshot: PromptQueueSnapshot) => void>()
vi.mock("@/lib/platform", () => ({
  subscribe: vi.fn(
    async (
      _event: string,
      handler: (snapshot: PromptQueueSnapshot) => void
    ) => {
      eventHandlers.add(handler)
      return () => eventHandlers.delete(handler)
    }
  ),
  onTransportReconnect: vi.fn(() => () => {}),
}))

import { useMessageQueue } from "./use-message-queue"

function draft(text: string): PromptDraft {
  return { blocks: [{ type: "text", text }], displayText: text }
}

function snapshot(
  conversationId: number,
  revision: number,
  rows: Array<{
    id: string
    text: string
    state?: "queued" | "claimed" | "paused"
  }> = []
): PromptQueueSnapshot {
  return {
    conversationId,
    revision,
    pausedReason: null,
    items: rows.map((row, position) => ({
      id: row.id,
      conversationId,
      position,
      draft: draft(row.text),
      originEventId: null,
      modeId: null,
      state: row.state ?? "queued",
      source: "user" as const,
      clientDedupeId: row.id,
      attempts: 0,
      pausedReason: null,
      createdAt: "2026-08-16T00:00:00Z",
      updatedAt: "2026-08-16T00:00:00Z",
    })),
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  eventHandlers.clear()
  api.get.mockResolvedValue(snapshot(7, 0))
  api.enqueue.mockImplementation(
    (input: { conversationId: number; id: string }) =>
      Promise.resolve(
        snapshot(input.conversationId, 1, [{ id: input.id, text: "A" }])
      )
  )
})

function emit(snapshot: PromptQueueSnapshot) {
  for (const handler of eventHandlers) handler(snapshot)
}

describe("useMessageQueue backend-authoritative behavior", () => {
  it("hydrates an existing Session and applies a newer cross-view snapshot", async () => {
    api.get.mockResolvedValue(snapshot(7, 2, [{ id: "a", text: "A" }]))
    const { result } = renderHook(() => useMessageQueue(7))
    await waitFor(() => expect(result.current.hydrated).toBe(true))
    expect(result.current.queue.map((item) => item.draft?.displayText)).toEqual(
      ["A"]
    )

    act(() => emit(snapshot(7, 3, [{ id: "b", text: "B" }])))
    expect(result.current.revision).toBe(3)
    expect(result.current.queue.map((item) => item.id)).toEqual(["b"])
  })

  it("does not let a stale response overwrite a newer worker event", async () => {
    api.get.mockResolvedValue(snapshot(7, 1, [{ id: "a", text: "A" }]))
    const { result } = renderHook(() => useMessageQueue(7))
    await waitFor(() => expect(result.current.hydrated).toBe(true))
    act(() => emit(snapshot(7, 5, [])))
    act(() => emit(snapshot(7, 4, [{ id: "old", text: "old" }])))
    expect(result.current.revision).toBe(5)
    expect(result.current.queue).toEqual([])
  })

  it("does not let a slow initial fetch overwrite an event that arrived first", async () => {
    let resolve!: (value: PromptQueueSnapshot) => void
    api.get.mockImplementation(
      () => new Promise<PromptQueueSnapshot>((done) => (resolve = done))
    )
    const { result } = renderHook(() => useMessageQueue(7))
    await waitFor(() => expect(eventHandlers.size).toBe(1))

    act(() => emit(snapshot(7, 3, [{ id: "new", text: "new" }])))
    expect(result.current.revision).toBe(3)
    await act(async () => resolve(snapshot(7, 1, [{ id: "old", text: "old" }])))
    expect(result.current.revision).toBe(3)
    expect(result.current.queue.map((item) => item.id)).toEqual(["new"])
  })

  it("shows an enqueue synchronously and uses one stable id for persistence", async () => {
    let resolve!: (value: PromptQueueSnapshot) => void
    api.enqueue.mockImplementation(
      () => new Promise<PromptQueueSnapshot>((done) => (resolve = done))
    )
    const { result } = renderHook(() => useMessageQueue(7))
    await waitFor(() => expect(result.current.hydrated).toBe(true))

    act(() => result.current.enqueue(draft("A"), null))
    expect(result.current.getQueueLength()).toBe(1)
    const id = result.current.queue[0].id
    expect(api.enqueue).toHaveBeenCalledWith(
      expect.objectContaining({ id, clientDedupeId: id, conversationId: 7 })
    )

    await act(async () => resolve(snapshot(7, 1, [{ id, text: "A" }])))
    expect(result.current.queue.map((item) => item.id)).toEqual([id])
  })

  it("returns the draft to its owner when durable enqueue fails", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {})
    const onPersistFailure = vi.fn()
    api.enqueue.mockRejectedValue(new Error("disk full"))
    const { result } = renderHook(() =>
      useMessageQueue(7, { onPersistFailure })
    )
    await waitFor(() => expect(result.current.hydrated).toBe(true))

    const failedDraft = draft("do not lose me")
    act(() => result.current.enqueue(failedDraft, null))
    await waitFor(() =>
      expect(onPersistFailure).toHaveBeenCalledWith(
        failedDraft,
        expect.objectContaining({ message: "disk full" })
      )
    )
    consoleError.mockRestore()
  })

  it("promotes a locally queued new-Session draft once its DB id arrives", async () => {
    const { result, rerender } = renderHook(
      ({ id }: { id: number | null }) => useMessageQueue(id),
      { initialProps: { id: null as number | null } }
    )
    act(() => result.current.enqueue(draft("A"), "plan"))
    const stableId = result.current.queue[0].id
    expect(api.enqueue).not.toHaveBeenCalled()

    rerender({ id: 7 })
    await waitFor(() => expect(api.enqueue).toHaveBeenCalledTimes(1))
    expect(api.enqueue).toHaveBeenCalledWith(
      expect.objectContaining({
        id: stableId,
        clientDedupeId: stableId,
        conversationId: 7,
        modeId: "plan",
      })
    )
  })

  it("rejects a stale reorder before it can drop or resurrect an item", async () => {
    api.get.mockResolvedValue(
      snapshot(7, 2, [
        { id: "a", text: "A" },
        { id: "b", text: "B" },
      ])
    )
    const { result } = renderHook(() => useMessageQueue(7))
    await waitFor(() => expect(result.current.queue).toHaveLength(2))
    const stale = [...result.current.queue].reverse()
    act(() => emit(snapshot(7, 3, [{ id: "b", text: "B" }])))
    act(() => result.current.reorder(stale))
    expect(result.current.queue.map((item) => item.id)).toEqual(["b"])
    expect(api.reorder).not.toHaveBeenCalled()
  })

  it("keeps two views of the same Session on one backend revision", async () => {
    api.get.mockResolvedValue(snapshot(7, 1, [{ id: "a", text: "A" }]))
    const first = renderHook(() => useMessageQueue(7))
    const second = renderHook(() => useMessageQueue(7))
    await waitFor(() => {
      expect(first.result.current.hydrated).toBe(true)
      expect(second.result.current.hydrated).toBe(true)
      expect(eventHandlers.size).toBe(2)
    })

    act(() => emit(snapshot(7, 2, [{ id: "b", text: "B" }])))
    expect(first.result.current.queue.map((item) => item.id)).toEqual(["b"])
    expect(second.result.current.queue.map((item) => item.id)).toEqual(["b"])
    expect(first.result.current.revision).toBe(2)
    expect(second.result.current.revision).toBe(2)
  })

  it("persists manual review and one-item release against the current revision", async () => {
    api.get.mockResolvedValue(snapshot(7, 4, [{ id: "a", text: "A" }]))
    api.pauseManual.mockResolvedValue({
      ...snapshot(7, 5, [{ id: "a", text: "A" }]),
      pausedReason: "manual_review",
    })
    api.releaseOne.mockResolvedValue({
      ...snapshot(7, 6, [{ id: "a", text: "A" }]),
      pausedReason: "manual_review",
      manualReleaseItemId: "a",
    })
    const { result } = renderHook(() => useMessageQueue(7))
    await waitFor(() => expect(result.current.revision).toBe(4))

    act(() => result.current.pauseManual())
    await waitFor(() =>
      expect(result.current.pausedReason).toBe("manual_review")
    )
    expect(api.pauseManual).toHaveBeenCalledWith(7, 4)

    act(() => result.current.releaseOne("a"))
    await waitFor(() => expect(result.current.manualReleaseItemId).toBe("a"))
    expect(api.releaseOne).toHaveBeenCalledWith(7, "a", 5)
  })

  it("does not carry optimistic queue items across a real Session switch", async () => {
    let resolve!: (value: PromptQueueSnapshot) => void
    api.enqueue.mockImplementation(
      () => new Promise<PromptQueueSnapshot>((done) => (resolve = done))
    )
    api.get.mockImplementation((id: number) =>
      Promise.resolve(snapshot(id, id === 7 ? 1 : 4))
    )
    const { result, rerender } = renderHook(
      ({ id }: { id: number | null }) => useMessageQueue(id),
      { initialProps: { id: 7 as number | null } }
    )
    await waitFor(() => expect(result.current.hydrated).toBe(true))
    act(() => result.current.enqueue(draft("belongs to seven"), null))
    expect(result.current.queue).toHaveLength(1)

    rerender({ id: 8 })
    await waitFor(() => {
      expect(result.current.hydrated).toBe(true)
      expect(result.current.queue).toEqual([])
    })
    await act(async () => resolve(snapshot(7, 2, [])))
    expect(result.current.queue).toEqual([])
    expect(result.current.revision).toBe(4)
  })
})
