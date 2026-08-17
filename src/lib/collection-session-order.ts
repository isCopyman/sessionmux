import type { CollectionInfo, DbConversationSummary } from "@/lib/types"

export function visibleCollectionSessionIds(args: {
  pathRoots: readonly { id: number }[]
  childrenByParent: Map<number | null, CollectionInfo[]>
  conversationsByCollection: Map<number, readonly DbConversationSummary[]>
  unclassifiedByRoot: Map<number, readonly DbConversationSummary[]>
  expanded: ReadonlySet<number>
  collapsedPaths: ReadonlySet<number>
  collapsedUnclassified: ReadonlySet<number>
}): number[] {
  const ids: number[] = []

  const walkCollections = (
    parent: number | null,
    rootFolderId: number | null
  ) => {
    for (const item of args.childrenByParent.get(parent) ?? []) {
      if ((item.root_folder_id ?? null) !== rootFolderId) continue
      if (!args.expanded.has(item.id)) continue
      for (const session of args.conversationsByCollection.get(item.id) ?? []) {
        ids.push(session.id)
      }
      walkCollections(item.id, rootFolderId)
    }
  }

  for (const root of args.pathRoots) {
    if (args.collapsedPaths.has(root.id)) continue
    walkCollections(null, root.id)
    if (!args.collapsedUnclassified.has(root.id)) {
      for (const session of args.unclassifiedByRoot.get(root.id) ?? []) {
        ids.push(session.id)
      }
    }
  }

  walkCollections(null, null)
  return ids
}
