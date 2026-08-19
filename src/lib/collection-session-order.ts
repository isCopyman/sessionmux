import type {
  CollaborationRoomSummary,
  CollectionInfo,
  DbConversationSummary,
} from "@/lib/types"
import { roomItemKey, sessionItemKey } from "@/lib/sidebar-item-selection"

/**
 * Mixed Session/Room visibility order for the Collection tree's multi-select:
 * a top-down walk in which every bucket emits its Sessions and then its Rooms,
 * matching `renderItems` / `renderUnclassified` in collection-tree.tsx. Keys
 * are the prefixed selection keys from `sidebar-item-selection`.
 */
export function visibleCollectionItemKeys(args: {
  pathRoots: readonly { id: number }[]
  childrenByParent: Map<number | null, CollectionInfo[]>
  conversationsByCollection: Map<number, readonly DbConversationSummary[]>
  unclassifiedByRoot: Map<number, readonly DbConversationSummary[]>
  roomsByCollection: Map<number, readonly CollaborationRoomSummary[]>
  roomsByUnclassifiedRoot: Map<number, readonly CollaborationRoomSummary[]>
  expanded: ReadonlySet<number>
  collapsedPaths: ReadonlySet<number>
  collapsedUnclassified: ReadonlySet<number>
}): string[] {
  const keys: string[] = []

  const walkCollections = (
    parent: number | null,
    rootFolderId: number | null
  ) => {
    for (const item of args.childrenByParent.get(parent) ?? []) {
      if ((item.root_folder_id ?? null) !== rootFolderId) continue
      if (!args.expanded.has(item.id)) continue
      for (const session of args.conversationsByCollection.get(item.id) ?? []) {
        keys.push(sessionItemKey(session.id))
      }
      for (const room of args.roomsByCollection.get(item.id) ?? []) {
        keys.push(roomItemKey(room.id))
      }
      walkCollections(item.id, rootFolderId)
    }
  }

  for (const root of args.pathRoots) {
    if (args.collapsedPaths.has(root.id)) continue
    walkCollections(null, root.id)
    if (!args.collapsedUnclassified.has(root.id)) {
      for (const session of args.unclassifiedByRoot.get(root.id) ?? []) {
        keys.push(sessionItemKey(session.id))
      }
      for (const room of args.roomsByUnclassifiedRoot.get(root.id) ?? []) {
        keys.push(roomItemKey(room.id))
      }
    }
  }

  walkCollections(null, null)
  return keys
}
