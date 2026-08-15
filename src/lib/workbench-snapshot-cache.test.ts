import { describe, expect, it } from "vitest"
import type { OpenedTabsSnapshot } from "@/lib/types"
import { WorkbenchSnapshotStore } from "@/lib/workbench-snapshot-cache"

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

describe("WorkbenchSnapshotStore", () => {
  it("keeps lightweight snapshots for every visited workbench", () => {
    const cache = new WorkbenchSnapshotStore()
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

    cache.set({
      workbenchId: 3,
      snapshot: snapshot(3, 33),
      connectionContextKeys: ["tab-3"],
      runtimeConversationIdByTab: {},
    })

    expect(cache.peek(1)).not.toBeNull()
    expect(cache.peek(2)).not.toBeNull()
    expect(cache.peek(3)).not.toBeNull()
  })

  it("does not replace a newer entry with a late older response", () => {
    const cache = new WorkbenchSnapshotStore()
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
    const cache = new WorkbenchSnapshotStore()
    cache.set({
      workbenchId: 1,
      snapshot: snapshot(1, 11),
      connectionContextKeys: ["tab-1"],
      runtimeConversationIdByTab: { "1:codex:11": -7 },
    })

    const read = cache.get(1)!
    read.snapshot.items[0].conversation_id = 99

    expect(cache.peek(1)?.snapshot.items[0].conversation_id).toBe(11)
    cache.deleteRuntimeConversationIds(new Set([-7]))
    expect(cache.peek(1)?.runtimeConversationIdByTab).toEqual({})
  })

  it("keeps reading positions until the Session warm cache evicts them", () => {
    const cache = new WorkbenchSnapshotStore()
    cache.set({
      workbenchId: 1,
      snapshot: snapshot(1, 11),
      connectionContextKeys: ["tab-1"],
      runtimeConversationIdByTab: {},
    })

    expect(
      cache.setSessionViewState(1, "tab-1", {
        scrollOffset: 720,
        atBottom: false,
        virtualItemCount: 9,
        virtualizerCache: null,
      })
    ).toBe(true)
    expect(cache.getSessionViewState(1, "tab-1")).toEqual({
      scrollOffset: 720,
      atBottom: false,
      virtualItemCount: 9,
      virtualizerCache: null,
    })
    expect(
      cache.setSessionViewState(1, "not-owned", {
        scrollOffset: 1,
        atBottom: false,
        virtualItemCount: 1,
        virtualizerCache: null,
      })
    ).toBe(false)

    cache.set({
      workbenchId: 2,
      snapshot: snapshot(2, 22),
      connectionContextKeys: ["tab-2"],
      runtimeConversationIdByTab: {},
    })
    expect(cache.getSessionViewState(1, "tab-1")).not.toBeNull()
    cache.deleteSessionViewStates(new Set(["tab-1"]))
    expect(cache.getSessionViewState(1, "tab-1")).toBeNull()
  })
})
