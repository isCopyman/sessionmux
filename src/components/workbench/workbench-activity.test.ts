import { describe, expect, it } from "vitest"

import type { ConnectionStatus } from "@/lib/types"
import {
  connectionKeysForSession,
  deriveWorkbenchActivity,
  sessionActivityFromStatus,
  type WorkbenchActivityTab,
  type WorkbenchConnectionSnapshot,
} from "./workbench-activity"

function tab(
  workbenchId: number,
  conversationId: number | null
): WorkbenchActivityTab {
  return { workbenchId, conversationId }
}

function connections(
  entries: Array<[number, ConnectionStatus]>
): Map<number, WorkbenchConnectionSnapshot> {
  return new Map(entries.map(([id, status]) => [id, { status }]))
}

describe("sessionActivityFromStatus", () => {
  it("maps prompting to busy and error to attention", () => {
    expect(sessionActivityFromStatus("prompting")).toBe("busy")
    expect(sessionActivityFromStatus("error")).toBe("attention")
  })

  it("treats connecting and idle statuses as no activity", () => {
    expect(sessionActivityFromStatus("connecting")).toBeNull()
    expect(sessionActivityFromStatus("connected")).toBeNull()
    expect(sessionActivityFromStatus("disconnected")).toBeNull()
    expect(sessionActivityFromStatus(null)).toBeNull()
    expect(sessionActivityFromStatus(undefined)).toBeNull()
  })
})

describe("connectionKeysForSession", () => {
  it("returns no keys for a draft without a conversation id", () => {
    expect(
      connectionKeysForSession({
        conversationId: null,
        liveTabId: "new-1",
        folderId: 7,
        agentType: "codex",
      })
    ).toEqual([])
  })

  it("prefers the live tab id and falls back to the canonical key", () => {
    expect(
      connectionKeysForSession({
        conversationId: 101,
        liveTabId: "conversation:101",
        folderId: 7,
        agentType: "codex",
      })
    ).toEqual(["conversation:101", "conv-7-codex-101"])
  })

  it("does not duplicate the canonical key when it is already the live id", () => {
    expect(
      connectionKeysForSession({
        conversationId: 101,
        liveTabId: "conv-7-codex-101",
        folderId: 7,
        agentType: "codex",
      })
    ).toEqual(["conv-7-codex-101"])
  })
})

describe("deriveWorkbenchActivity", () => {
  it("returns empty maps for empty inputs", () => {
    const result = deriveWorkbenchActivity([], new Map())
    expect(result.byConversationId.size).toBe(0)
    expect(result.busyCountByWorkbenchId.size).toBe(0)
  })

  it("derives per-session busy and attention from connection status", () => {
    const result = deriveWorkbenchActivity(
      [tab(1, 101), tab(1, 102), tab(1, 103), tab(1, 104)],
      connections([
        [101, "prompting"],
        [102, "error"],
        [103, "connecting"],
        [104, "connected"],
      ])
    )

    expect(result.byConversationId.get(101)).toBe("busy")
    expect(result.byConversationId.get(102)).toBe("attention")
    expect(result.byConversationId.has(103)).toBe(false)
    expect(result.byConversationId.has(104)).toBe(false)
    expect(result.busyCountByWorkbenchId.get(1)).toBe(1)
  })

  it("skips drafts and rooms that have no conversation id", () => {
    const result = deriveWorkbenchActivity(
      [tab(1, null), tab(1, 101), tab(1, null)],
      connections([[101, "prompting"]])
    )

    expect(result.byConversationId.get(101)).toBe("busy")
    expect(result.busyCountByWorkbenchId.get(1)).toBe(1)
  })

  it("does not count attention sessions in the workbench busy total", () => {
    const result = deriveWorkbenchActivity(
      [tab(1, 101), tab(1, 102)],
      connections([
        [101, "error"],
        [102, "disconnected"],
      ])
    )

    expect(result.byConversationId.get(101)).toBe("attention")
    expect(result.busyCountByWorkbenchId.get(1)).toBeUndefined()
  })

  it("counts a busy session on every workbench it belongs to", () => {
    const result = deriveWorkbenchActivity(
      [tab(1, 101), tab(2, 101), tab(2, 202)],
      connections([
        [101, "prompting"],
        [202, "prompting"],
      ])
    )

    expect(result.busyCountByWorkbenchId.get(1)).toBe(1)
    expect(result.busyCountByWorkbenchId.get(2)).toBe(2)
  })

  it("counts a duplicated tab on the same workbench once", () => {
    const result = deriveWorkbenchActivity(
      [tab(1, 101), tab(1, 101), tab(1, 102)],
      connections([
        [101, "prompting"],
        [102, "prompting"],
      ])
    )

    expect(result.busyCountByWorkbenchId.get(1)).toBe(2)
  })

  it("ignores connections that no tab references", () => {
    const result = deriveWorkbenchActivity(
      [tab(1, 101)],
      connections([
        [101, "prompting"],
        [999, "prompting"],
      ])
    )

    expect(result.byConversationId.has(999)).toBe(false)
    expect(result.busyCountByWorkbenchId.get(1)).toBe(1)
  })
})
