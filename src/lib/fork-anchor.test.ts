import { describe, expect, it } from "vitest"
import {
  ForkAnchorRejectedError,
  FORK_ANCHOR_REJECTED_CODE,
  FORK_ANCHOR_REJECTED_MARKER,
  buildAcpForkArgs,
  canShowForkAtMessage,
  isForkAnchorRejection,
  lastProviderAnchor,
  nonemptyProviderAnchor,
} from "./fork-anchor"
import { TurnBusyError, isTurnInProgressRejection } from "./turn-busy"

describe("buildAcpForkArgs", () => {
  it("omits anchor for a head fork so the payload matches today's shape", () => {
    expect(
      buildAcpForkArgs({
        connectionId: "c1",
        conversationId: 12,
        folderId: 3,
      })
    ).toEqual({
      connectionId: "c1",
      conversationId: 12,
      folderId: 3,
    })
    expect(
      buildAcpForkArgs({
        connectionId: "c1",
        conversationId: null,
        folderId: null,
        anchor: null,
      })
    ).toEqual({
      connectionId: "c1",
      conversationId: null,
      folderId: null,
    })
    expect(
      buildAcpForkArgs({
        connectionId: "c1",
        anchor: "   ",
      })
    ).not.toHaveProperty("anchor")
  })

  it("includes a non-empty anchor for forkAtMessage", () => {
    expect(
      buildAcpForkArgs({
        connectionId: "c1",
        conversationId: 12,
        folderId: 3,
        anchor: "u-user",
      })
    ).toEqual({
      connectionId: "c1",
      conversationId: 12,
      folderId: 3,
      anchor: "u-user",
    })
  })
})

describe("canShowForkAtMessage", () => {
  it("requires claude_code AND a non-empty provider_anchor", () => {
    expect(canShowForkAtMessage("claude_code", "u-user")).toBe(true)
    expect(canShowForkAtMessage("claude_code", "  ")).toBe(false)
    expect(canShowForkAtMessage("claude_code", null)).toBe(false)
    expect(canShowForkAtMessage("claude_code", undefined)).toBe(false)
    expect(canShowForkAtMessage("codex", "u-user")).toBe(false)
    expect(canShowForkAtMessage("gemini", "u-user")).toBe(false)
    expect(canShowForkAtMessage(null, "u-user")).toBe(false)
  })
})

describe("lastProviderAnchor", () => {
  it("returns the last non-empty provider_anchor in the group", () => {
    expect(lastProviderAnchor(undefined)).toBeNull()
    expect(lastProviderAnchor([])).toBeNull()
    expect(lastProviderAnchor([{ provider_anchor: "u-user" }])).toBe("u-user")
    expect(
      lastProviderAnchor([
        { provider_anchor: "u-user" },
        { provider_anchor: "  " },
        { provider_anchor: "att-tokens" },
      ])
    ).toBe("att-tokens")
  })
})

describe("isForkAnchorRejection", () => {
  it("recognizes the CLI prefix, stable code, and typed error", () => {
    expect(
      isForkAnchorRejection(
        `${FORK_ANCHOR_REJECTED_MARKER} kept turn ended at tool_result`
      )
    ).toBe(true)
    expect(
      isForkAnchorRejection({
        code: FORK_ANCHOR_REJECTED_CODE,
        message: "rejected",
      })
    ).toBe(true)
    expect(
      isForkAnchorRejection({
        message: `${FORK_ANCHOR_REJECTED_MARKER} x`,
      })
    ).toBe(true)
    expect(isForkAnchorRejection(new ForkAnchorRejectedError())).toBe(true)
  })

  it("does not treat turn-in-progress as an anchor rejection", () => {
    expect(isForkAnchorRejection("turn already in progress")).toBe(false)
    expect(isForkAnchorRejection(new TurnBusyError())).toBe(false)
    expect(isTurnInProgressRejection(new ForkAnchorRejectedError())).toBe(false)
  })

  it("returns false for unrelated errors", () => {
    expect(isForkAnchorRejection("process exited unexpectedly")).toBe(false)
    expect(isForkAnchorRejection(null)).toBe(false)
    expect(isForkAnchorRejection(undefined)).toBe(false)
    expect(isForkAnchorRejection({})).toBe(false)
  })
})

describe("nonemptyProviderAnchor", () => {
  it("trims and rejects blanks", () => {
    expect(nonemptyProviderAnchor("  u-user  ")).toBe("u-user")
    expect(nonemptyProviderAnchor("")).toBeNull()
    expect(nonemptyProviderAnchor(null)).toBeNull()
  })
})
