import { beforeEach, describe, expect, it, vi } from "vitest"

import { applyPendingClaudeProfileAndRespawn } from "./apply-pending-claude-profile"

const api = vi.hoisted(() => ({
  conversationSetClaudeProfile: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  conversationSetClaudeProfile: (...args: unknown[]) =>
    api.conversationSetClaudeProfile(...args),
}))

describe("applyPendingClaudeProfileAndRespawn", () => {
  beforeEach(() => {
    api.conversationSetClaudeProfile.mockReset()
    api.conversationSetClaudeProfile.mockResolvedValue({
      conversationId: 12,
      profileId: "api",
      affectedRunningSessions: 0,
    })
  })

  it("is a no-op when the composer has no pending choice", async () => {
    const disconnect = vi.fn()
    const connect = vi.fn()
    const wrote = await applyPendingClaudeProfileAndRespawn({
      conversationId: 12,
      pendingProfileId: null,
      disconnect,
      connect,
    })
    expect(wrote).toBe(false)
    expect(api.conversationSetClaudeProfile).not.toHaveBeenCalled()
    expect(disconnect).not.toHaveBeenCalled()
    expect(connect).not.toHaveBeenCalled()
  })

  it("writes the pending profile then respawns before returning", async () => {
    const order: string[] = []
    api.conversationSetClaudeProfile.mockImplementation(async () => {
      order.push("write")
      return {
        conversationId: 12,
        profileId: "api",
        affectedRunningSessions: 0,
      }
    })
    const disconnect = vi.fn(async () => {
      order.push("disconnect")
    })
    const connect = vi.fn(async (conversationId: number) => {
      order.push(`connect:${conversationId}`)
    })

    const wrote = await applyPendingClaudeProfileAndRespawn({
      conversationId: 12,
      pendingProfileId: "api",
      disconnect,
      connect,
    })

    expect(wrote).toBe(true)
    expect(api.conversationSetClaudeProfile).toHaveBeenCalledWith(12, "api")
    expect(order).toEqual(["write", "disconnect", "connect:12"])
  })
})
