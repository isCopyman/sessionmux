import { describe, expect, it } from "vitest"

import type {
  CollaborationRoomSummary,
  CollectionInfo,
  DbConversationSummary,
} from "@/lib/types"
import { visibleCollectionItemKeys } from "./collection-session-order"

function collection(
  id: number,
  parentId: number | null,
  rootFolderId: number | null
): CollectionInfo {
  return {
    id,
    parent_id: parentId,
    root_folder_id: rootFolderId,
    name: `c${id}`,
    position: 0,
    created_at: "2026-06-01T00:00:00.000Z",
    updated_at: "2026-06-01T00:00:00.000Z",
  }
}

function session(id: number): DbConversationSummary {
  return {
    id,
    folder_id: 7,
    title: `s${id}`,
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
    archived_at: null,
    pinned_at: null,
  }
}

function room(id: string): CollaborationRoomSummary {
  return {
    id,
    workbenchId: 1,
    title: `Room ${id}`,
    createdByConversationId: 101,
    memberCount: 2,
    unreadCount: 0,
    createdAt: "2026-06-01T00:00:00.000Z",
    updatedAt: "2026-06-01T00:00:00.000Z",
  }
}

describe("visibleCollectionItemKeys", () => {
  const research = collection(10, null, 7)
  const sources = collection(11, 10, 7)
  const childrenByParent = new Map<number | null, CollectionInfo[]>([
    [null, [research]],
    [10, [sources]],
  ])
  const conversationsByCollection = new Map([
    [10, [session(101)]],
    [11, [session(102)]],
  ])
  const unclassifiedByRoot = new Map([[7, [session(201)]]])
  const roomsByCollection = new Map([
    [10, [room("rm_a")]],
    [11, [room("rm_b")]],
  ])
  const roomsByUnclassifiedRoot = new Map([[7, [room("rm_c")]]])

  const baseArgs = {
    pathRoots: [{ id: 7 }],
    childrenByParent,
    conversationsByCollection,
    unclassifiedByRoot,
    roomsByCollection,
    roomsByUnclassifiedRoot,
    collapsedPaths: new Set<number>(),
    collapsedUnclassified: new Set<number>(),
  }

  it("emits a bucket's Rooms right after its Sessions, matching render order", () => {
    expect(
      visibleCollectionItemKeys({ ...baseArgs, expanded: new Set([10, 11]) })
    ).toEqual([
      "session:101",
      "room:rm_a",
      "session:102",
      "room:rm_b",
      "session:201",
      "room:rm_c",
    ])
  })

  it("skips Rooms inside collapsed collections, like their Sessions", () => {
    expect(
      visibleCollectionItemKeys({ ...baseArgs, expanded: new Set() })
    ).toEqual(["session:201", "room:rm_c"])
  })

  it("hides a collapsed path's Sessions and Rooms alike", () => {
    expect(
      visibleCollectionItemKeys({
        ...baseArgs,
        expanded: new Set([10, 11]),
        collapsedPaths: new Set([7]),
      })
    ).toEqual([])
  })
})
