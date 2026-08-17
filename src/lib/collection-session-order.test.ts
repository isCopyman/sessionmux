import { describe, expect, it } from "vitest"

import type { CollectionInfo, DbConversationSummary } from "@/lib/types"
import { visibleCollectionSessionIds } from "./collection-session-order"

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

describe("visibleCollectionSessionIds", () => {
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
  const unclassifiedByRoot = new Map([[7, [session(201), session(202)]]])

  it("lists unclassified sessions when the path is open and collections are collapsed", () => {
    expect(
      visibleCollectionSessionIds({
        pathRoots: [{ id: 7 }],
        childrenByParent,
        conversationsByCollection,
        unclassifiedByRoot,
        expanded: new Set(),
        collapsedPaths: new Set(),
        collapsedUnclassified: new Set(),
      })
    ).toEqual([201, 202])
  })

  it("walks expanded collections before unclassified, matching the tree", () => {
    expect(
      visibleCollectionSessionIds({
        pathRoots: [{ id: 7 }],
        childrenByParent,
        conversationsByCollection,
        unclassifiedByRoot,
        expanded: new Set([10, 11]),
        collapsedPaths: new Set(),
        collapsedUnclassified: new Set(),
      })
    ).toEqual([101, 102, 201, 202])
  })

  it("hides a collapsed path entirely", () => {
    expect(
      visibleCollectionSessionIds({
        pathRoots: [{ id: 7 }],
        childrenByParent,
        conversationsByCollection,
        unclassifiedByRoot,
        expanded: new Set([10, 11]),
        collapsedPaths: new Set([7]),
        collapsedUnclassified: new Set(),
      })
    ).toEqual([])
  })
})
