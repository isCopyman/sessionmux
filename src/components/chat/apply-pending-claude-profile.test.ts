import { beforeEach, describe, expect, it, vi } from "vitest"

import { persistPendingClaudeProfile } from "./apply-pending-claude-profile"

const api = vi.hoisted(() => ({
  conversationSetClaudeProfile: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  conversationSetClaudeProfile: (...args: unknown[]) =>
    api.conversationSetClaudeProfile(...args),
}))

describe("persistPendingClaudeProfile", () => {
  beforeEach(() => {
    api.conversationSetClaudeProfile.mockReset()
    api.conversationSetClaudeProfile.mockResolvedValue({
      conversationId: 12,
      profileId: "api",
      affectedRunningSessions: 0,
    })
  })

  it("is a no-op when the composer has no pending choice", async () => {
    const wrote = await persistPendingClaudeProfile({
      conversationId: 12,
      pendingProfileId: null,
    })
    expect(wrote).toBe(false)
    expect(api.conversationSetClaudeProfile).not.toHaveBeenCalled()
  })

  it("records the already-applied profile without owning process restart", async () => {
    const order: string[] = []
    api.conversationSetClaudeProfile.mockImplementation(async () => {
      order.push("write")
      return {
        conversationId: 12,
        profileId: "api",
        affectedRunningSessions: 0,
      }
    })
    const wrote = await persistPendingClaudeProfile({
      conversationId: 12,
      pendingProfileId: "api",
    })

    expect(wrote).toBe(true)
    expect(api.conversationSetClaudeProfile).toHaveBeenCalledWith(12, "api")
    expect(order).toEqual(["write"])
  })
})
