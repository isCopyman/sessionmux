import { beforeEach, describe, expect, it, vi } from "vitest"
import {
  DEFAULT_SESSION_WARM_CACHE_LIMIT,
  MAX_SESSION_WARM_CACHE_LIMIT,
  readSessionWarmCacheLimit,
  SESSION_WARM_CACHE_LIMIT_STORAGE_KEY,
  subscribeSessionWarmCacheLimit,
  writeSessionWarmCacheLimit,
} from "@/lib/session-warm-cache-settings"

describe("session warm cache settings", () => {
  beforeEach(() => localStorage.clear())

  it("defaults to eight Sessions and clamps persisted input", () => {
    expect(readSessionWarmCacheLimit()).toBe(DEFAULT_SESSION_WARM_CACHE_LIMIT)
    expect(writeSessionWarmCacheLimit(-4)).toBe(0)
    expect(readSessionWarmCacheLimit()).toBe(0)
    expect(writeSessionWarmCacheLimit(500)).toBe(MAX_SESSION_WARM_CACHE_LIMIT)
  })

  it("notifies the current WebView when the setting changes", () => {
    const listener = vi.fn()
    const unsubscribe = subscribeSessionWarmCacheLimit(listener)

    writeSessionWarmCacheLimit(12)

    expect(listener).toHaveBeenNthCalledWith(
      1,
      DEFAULT_SESSION_WARM_CACHE_LIMIT
    )
    expect(listener).toHaveBeenLastCalledWith(12)
    unsubscribe()
  })

  it("reacts to storage changes from another window", () => {
    const listener = vi.fn()
    const unsubscribe = subscribeSessionWarmCacheLimit(listener)
    localStorage.setItem(SESSION_WARM_CACHE_LIMIT_STORAGE_KEY, "20")

    window.dispatchEvent(
      new StorageEvent("storage", {
        key: SESSION_WARM_CACHE_LIMIT_STORAGE_KEY,
        newValue: "20",
      })
    )

    expect(listener).toHaveBeenLastCalledWith(20)
    unsubscribe()
  })
})
