import { describe, expect, it } from "vitest"
import type { OpenedTabsSnapshot } from "@/lib/types"
import { RecentWorkbenchSnapshotCache } from "@/lib/workbench-snapshot-cache"

function snapshot(version: number, conversationId: number): OpenedTabsSnapshot {
  return {
    version,
    items: [
      {
        id: conversationId,
        folder_id: 1,
        conversation_id: conversationId,
        agent_type: "codex",
        is_pinned: true,
        is_active: true,
        position: 0,
      },
    ],
  }
}

describe("RecentWorkbenchSnapshotCache", () => {
  it("evicts the least recently used workbench", () => {
    const cache = new RecentWorkbenchSnapshotCache(2)
    cache.set({
      workbenchId: 1,
      snapshot: snapshot(1, 11),
      connectionContextKeys: ["tab-1"],
      runtimeConversationIdByTab: {},
    })
    cache.set({
      workbenchId: 2,
      snapshot: snapshot(2, 22),
      connectionContextKeys: ["tab-2"],
      runtimeConversationIdByTab: {},
    })

    expect(cache.get(1)?.snapshot.items[0].conversation_id).toBe(11)
    const evicted = cache.set({
      workbenchId: 3,
      snapshot: snapshot(3, 33),
      connectionContextKeys: ["tab-3"],
      runtimeConversationIdByTab: {},
    })

    expect(evicted.map((entry) => entry.workbenchId)).toEqual([2])
    expect(cache.peek(1)).not.toBeNull()
    expect(cache.peek(2)).toBeNull()
  })

  it("does not replace a newer entry with a late older response", () => {
    const cache = new RecentWorkbenchSnapshotCache()
    cache.set({
      workbenchId: 1,
      snapshot: snapshot(5, 55),
      connectionContextKeys: ["tab-1"],
      runtimeConversationIdByTab: {},
    })
    cache.set({
      workbenchId: 1,
      snapshot: snapshot(4, 44),
      connectionContextKeys: ["tab-1"],
      runtimeConversationIdByTab: {},
    })

    expect(cache.peek(1)?.snapshot.items[0].conversation_id).toBe(55)
  })

  it("tracks virtual runtime ids and returns defensive copies", () => {
    const cache = new RecentWorkbenchSnapshotCache()
    cache.set({
      workbenchId: 1,
      snapshot: snapshot(1, 11),
      connectionContextKeys: ["tab-1"],
      runtimeConversationIdByTab: { "1:codex:11": -7 },
    })

    const read = cache.get(1)!
    read.snapshot.items[0].conversation_id = 99

    expect(cache.peek(1)?.snapshot.items[0].conversation_id).toBe(11)
    expect([...cache.retainedRuntimeConversationIds()]).toEqual([-7])
    expect([...cache.retainedConnectionContextKeys()]).toEqual(["tab-1"])
  })

  it("can shrink the warm set and evicts least-recently-used entries", () => {
    const cache = new RecentWorkbenchSnapshotCache(3)
    for (const id of [1, 2, 3]) {
      cache.set({
        workbenchId: id,
        snapshot: snapshot(id, id * 10),
        connectionContextKeys: [`tab-${id}`],
        runtimeConversationIdByTab: {},
      })
    }

    cache.get(1)
    const evicted = cache.setCapacity(1)

    expect(evicted.map((entry) => entry.workbenchId)).toEqual([2, 3])
    expect(cache.peek(1)?.workbenchId).toBe(1)
    expect([...cache.retainedConnectionContextKeys()]).toEqual(["tab-1"])
  })
})
