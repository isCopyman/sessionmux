import { describe, expect, it } from "vitest"

import type { DbConversationSummary } from "@/lib/types"
import { defaultRoomTitle, roomMemberCandidates } from "./room-create"

function conversation(
  id: number,
  title: string | null,
  archived = false
): DbConversationSummary {
  return {
    id,
    folder_id: 7,
    title,
    title_locked: true,
    agent_type: "codex",
    status: "in_progress",
    kind: "regular",
    model: null,
    git_branch: null,
    external_id: null,
    message_count: 1,
    child_count: 0,
    created_at: "2026-06-01T00:00:00.000Z",
    updated_at: "2026-06-01T00:00:00.000Z",
    archived_at: archived ? "2026-06-02T00:00:00.000Z" : null,
    pinned_at: null,
  }
}

describe("roomMemberCandidates", () => {
  const conversations = [
    conversation(1, "Evidence review"),
    conversation(2, "Loose notes"),
    conversation(3, "Old session", true),
  ]

  it("excludes archived Sessions", () => {
    expect(
      roomMemberCandidates(conversations, "").map((item) => item.id)
    ).toEqual([1, 2])
  })

  it("matches the query against title and id", () => {
    expect(
      roomMemberCandidates(conversations, "evidence").map((item) => item.id)
    ).toEqual([1])
    expect(
      roomMemberCandidates(conversations, "2").map((item) => item.id)
    ).toEqual([2])
  })
})

describe("defaultRoomTitle", () => {
  it("joins the first two member titles", () => {
    expect(
      defaultRoomTitle([{ title: "Alpha" }, { title: "Beta" }], "Room")
    ).toBe("Alpha / Beta")
  })

  it("adds an ellipsis when more than two members are in", () => {
    expect(
      defaultRoomTitle(
        [{ title: "Alpha" }, { title: "Beta" }, { title: "Gamma" }],
        "Room"
      )
    ).toBe("Alpha / Beta…")
  })

  it("falls back when no member has a title", () => {
    expect(defaultRoomTitle([{ title: null }, { title: null }], "Room")).toBe(
      "Room"
    )
  })
})
