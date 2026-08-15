import { describe, expect, it } from "vitest"
import { RecentSessionWarmCache } from "@/lib/session-warm-cache"

describe("RecentSessionWarmCache", () => {
  it("evicts Sessions by recency rather than by Workbench", () => {
    const cache = new RecentSessionWarmCache(2)
    cache.touch({
      runtimeConversationId: 11,
      connectionContextKeys: ["tab-11"],
    })
    cache.touch({
      runtimeConversationId: 22,
      connectionContextKeys: ["tab-22"],
    })
    cache.touch({
      runtimeConversationId: 11,
      connectionContextKeys: ["tab-11"],
    })

    const evicted = cache.touch({
      runtimeConversationId: 33,
      connectionContextKeys: ["tab-33"],
    })

    expect(evicted.map((entry) => entry.runtimeConversationId)).toEqual([22])
    expect([...cache.retainedRuntimeConversationIds()]).toEqual([11, 33])
  })

  it("merges surface keys when one runtime is referenced more than once", () => {
    const cache = new RecentSessionWarmCache(2)
    cache.touch({
      runtimeConversationId: -7,
      connectionContextKeys: ["new-tab"],
    })
    cache.touch({
      runtimeConversationId: -7,
      connectionContextKeys: ["bound-tab"],
    })

    expect([...cache.retainedRuntimeConversationIds()]).toEqual([-7])
    expect([...cache.retainedConnectionContextKeys()]).toEqual([
      "new-tab",
      "bound-tab",
    ])
  })

  it("can be disabled and returns every entry for cleanup", () => {
    const cache = new RecentSessionWarmCache(2)
    cache.touch({
      runtimeConversationId: 1,
      connectionContextKeys: ["tab-1"],
    })
    cache.touch({
      runtimeConversationId: 2,
      connectionContextKeys: ["tab-2"],
    })

    const evicted = cache.setCapacity(0)

    expect(evicted.map((entry) => entry.runtimeConversationId)).toEqual([1, 2])
    expect(cache.hasConnectionContextKey("tab-1")).toBe(false)
  })

  it("temporarily exceeds the limit instead of evicting a running Session", () => {
    const cache = new RecentSessionWarmCache(1)
    cache.touch({
      runtimeConversationId: 1,
      connectionContextKeys: ["tab-1"],
    })

    const evicted = cache.touch(
      { runtimeConversationId: 2, connectionContextKeys: ["tab-2"] },
      new Set([1, 2])
    )

    expect(evicted).toEqual([])
    expect([...cache.retainedRuntimeConversationIds()]).toEqual([1, 2])
    expect(cache.setCapacity(1, new Set([2]))).toEqual([
      { runtimeConversationId: 1, connectionContextKeys: ["tab-1"] },
    ])
  })

  it("rejects invalid capacities", () => {
    expect(() => new RecentSessionWarmCache(-1)).toThrow(/non-negative/)
    expect(() => new RecentSessionWarmCache(1.5)).toThrow(/non-negative/)
  })
})
