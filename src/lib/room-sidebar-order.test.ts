import { describe, expect, it } from "vitest"

import type { CollaborationRoomSummary } from "@/lib/types"

import { sortRoomsForSidebar } from "./room-sidebar-order"

function room(
  id: string,
  timestamps: {
    createdAt: string
    updatedAt: string
    lastEventAt?: string | null
  }
): CollaborationRoomSummary {
  return {
    id,
    workbenchId: 1,
    title: id,
    createdByConversationId: 1,
    memberCount: 2,
    unreadCount: 0,
    ...timestamps,
  }
}

describe("sortRoomsForSidebar", () => {
  it("keeps created order when a later open only bumps updatedAt", () => {
    const opened = room("rm_old", {
      createdAt: "2026-06-01T00:00:00.000Z",
      updatedAt: "2026-08-19T12:00:00.000Z",
      lastEventAt: "2026-06-01T01:00:00.000Z",
    })
    const quieter = room("rm_new", {
      createdAt: "2026-06-02T00:00:00.000Z",
      updatedAt: "2026-06-02T00:00:00.000Z",
      lastEventAt: "2026-06-02T00:00:00.000Z",
    })
    expect(
      sortRoomsForSidebar([opened, quieter], "created").map((item) => item.id)
    ).toEqual(["rm_new", "rm_old"])
  })

  it("uses lastEventAt, not a seen-cursor updatedAt, for recency", () => {
    const seen = room("rm_seen", {
      createdAt: "2026-06-01T00:00:00.000Z",
      updatedAt: "2026-08-19T12:00:00.000Z",
      lastEventAt: "2026-06-01T01:00:00.000Z",
    })
    const posted = room("rm_posted", {
      createdAt: "2026-06-01T00:00:00.000Z",
      updatedAt: "2026-06-03T00:00:00.000Z",
      lastEventAt: "2026-06-03T00:00:00.000Z",
    })
    expect(
      sortRoomsForSidebar([seen, posted], "updated").map((item) => item.id)
    ).toEqual(["rm_posted", "rm_seen"])
  })
})
