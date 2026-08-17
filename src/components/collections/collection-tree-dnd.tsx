"use client"

import { useCallback, type ReactNode } from "react"
import {
  useDraggable,
  useDroppable,
  type Data,
  type DraggableAttributes,
  type DraggableSyntheticListeners,
} from "@dnd-kit/core"
import type { CollectionInfo } from "@/lib/types"

export type SessionTreeDrag = {
  kind: "session"
  conversationId: number
  /** Sessions that will move together. Always includes `conversationId`. */
  conversationIds?: number[]
  rootFolderId: number
  label: string
}

export function sessionIdsInDrag(payload: SessionTreeDrag): number[] {
  const ids =
    payload.conversationIds && payload.conversationIds.length > 0
      ? payload.conversationIds
      : [payload.conversationId]
  const seen = new Set<number>()
  const result: number[] = []
  for (const id of ids) {
    if (seen.has(id)) continue
    seen.add(id)
    result.push(id)
  }
  if (!seen.has(payload.conversationId)) {
    result.unshift(payload.conversationId)
  }
  return result
}

export function sessionDragPayload(args: {
  grabbedId: number
  grabbedRootFolderId: number
  grabbedLabel: string
  selectedIds: readonly number[]
  rootFolderIdByConversation: ReadonlyMap<number, number>
}): SessionTreeDrag {
  const selectedOnPath = args.selectedIds.filter(
    (id) => args.rootFolderIdByConversation.get(id) === args.grabbedRootFolderId
  )
  const ids =
    selectedOnPath.includes(args.grabbedId) && selectedOnPath.length > 1
      ? selectedOnPath
      : [args.grabbedId]
  return {
    kind: "session",
    conversationId: args.grabbedId,
    conversationIds: ids,
    rootFolderId: args.grabbedRootFolderId,
    label: args.grabbedLabel,
  }
}

export type CollectionTreeDrag = {
  kind: "collection"
  collectionId: number
  rootFolderId: number | null
  label: string
}

export type TreeDragData = SessionTreeDrag | CollectionTreeDrag

export type TreeDropData =
  | {
      kind: "session-bucket"
      rootFolderId: number
      collectionId: number | null
    }
  | {
      kind: "collection-row"
      collectionId: number
      rootFolderId: number | null
    }
  | {
      kind: "collection-root"
      rootFolderId: number | null
    }

export type CollectionDropTarget = {
  targetCollectionId: number | null
  rootFolderId: number | null
  parentId: number | null
  index: number
  position: "before" | "inside" | "after" | "root"
}

export function canDropSessionOnTarget(
  payload: SessionTreeDrag,
  rootFolderId: number,
  collectionId: number | null,
  currentCollectionId: number | null,
  busy = false,
  membershipByConversation?: ReadonlyMap<number, number>
) {
  if (busy || payload.rootFolderId !== rootFolderId) return false
  return sessionIdsInDrag(payload).some((id) => {
    const current =
      membershipByConversation != null
        ? (membershipByConversation.get(id) ?? null)
        : id === payload.conversationId
          ? currentCollectionId
          : null
    return current !== collectionId
  })
}

function descendants(items: CollectionInfo[], id: number) {
  const result = new Set<number>([id])
  let changed = true
  while (changed) {
    changed = false
    for (const item of items) {
      if (
        item.parent_id != null &&
        result.has(item.parent_id) &&
        !result.has(item.id)
      ) {
        result.add(item.id)
        changed = true
      }
    }
  }
  return result
}

function siblingsAt(
  items: CollectionInfo[],
  parentId: number | null,
  rootFolderId: number | null,
  withoutId?: number
) {
  return items
    .filter(
      (item) =>
        item.parent_id === parentId &&
        (item.root_folder_id ?? null) === rootFolderId &&
        item.id !== withoutId
    )
    .sort((a, b) => a.position - b.position || a.id - b.id)
}

function collectionPlacementChanged(
  payload: CollectionTreeDrag,
  parentId: number | null,
  index: number,
  items: CollectionInfo[]
) {
  const moving = items.find((item) => item.id === payload.collectionId)
  if (!moving || moving.parent_id !== parentId) return true
  const currentIndex = siblingsAt(
    items,
    parentId,
    payload.rootFolderId
  ).findIndex((item) => item.id === payload.collectionId)
  return currentIndex !== index
}

export function collectionPlacementForRow(
  payload: CollectionTreeDrag,
  target: CollectionInfo,
  rect: Pick<DOMRect, "top" | "height">,
  clientY: number,
  items: CollectionInfo[],
  busy = false
): CollectionDropTarget | null {
  if (
    busy ||
    payload.rootFolderId !== (target.root_folder_id ?? null) ||
    target.id === payload.collectionId
  ) {
    return null
  }

  const ratio = rect.height > 0 ? (clientY - rect.top) / rect.height : 0.5
  const position = ratio < 0.25 ? "before" : ratio > 0.75 ? "after" : "inside"
  const invalidParents = descendants(items, payload.collectionId)

  if (position === "inside") {
    if (invalidParents.has(target.id)) return null
    const index = siblingsAt(
      items,
      target.id,
      payload.rootFolderId,
      payload.collectionId
    ).length
    if (!collectionPlacementChanged(payload, target.id, index, items)) {
      return null
    }
    return {
      targetCollectionId: target.id,
      rootFolderId: payload.rootFolderId,
      parentId: target.id,
      index,
      position,
    }
  }

  const parentId = target.parent_id
  if (parentId != null && invalidParents.has(parentId)) return null
  const siblings = siblingsAt(
    items,
    parentId,
    payload.rootFolderId,
    payload.collectionId
  )
  const targetIndex = siblings.findIndex((item) => item.id === target.id)
  if (targetIndex < 0) return null
  const index = targetIndex + (position === "after" ? 1 : 0)
  if (!collectionPlacementChanged(payload, parentId, index, items)) return null
  return {
    targetCollectionId: target.id,
    rootFolderId: payload.rootFolderId,
    parentId,
    index,
    position,
  }
}

export function collectionPlacementForRoot(
  payload: CollectionTreeDrag,
  rootFolderId: number | null,
  items: CollectionInfo[],
  busy = false
): CollectionDropTarget | null {
  if (busy || payload.rootFolderId !== rootFolderId) return null
  const index = siblingsAt(
    items,
    null,
    rootFolderId,
    payload.collectionId
  ).length
  if (!collectionPlacementChanged(payload, null, index, items)) return null
  return {
    targetCollectionId: null,
    rootFolderId,
    parentId: null,
    index,
    position: "root",
  }
}

type TreeDndBindingsProps = {
  dragId: string
  dragData: TreeDragData
  dropId: string
  dropData: TreeDropData
  disabled?: boolean
  children: (bindings: {
    setNodeRef: (node: HTMLElement | null) => void
    attributes: DraggableAttributes
    listeners: DraggableSyntheticListeners | undefined
    isDragging: boolean
    isOver: boolean
  }) => ReactNode
}

/**
 * Connect one rendered tree row to the single dnd-kit lifecycle. The row may
 * expose a smaller activator (for example its label button) while the full row
 * remains the measured drag source and drop target.
 */
export function TreeDndBindings({
  dragId,
  dragData,
  dropId,
  dropData,
  disabled = false,
  children,
}: TreeDndBindingsProps) {
  const draggable = useDraggable({
    id: dragId,
    data: dragData as Data,
    disabled,
  })
  const droppable = useDroppable({
    id: dropId,
    data: dropData as Data,
    disabled,
  })
  const setDraggableNodeRef = draggable.setNodeRef
  const setDroppableNodeRef = droppable.setNodeRef
  const setNodeRef = useCallback(
    (node: HTMLElement | null) => {
      setDraggableNodeRef(node)
      setDroppableNodeRef(node)
    },
    [setDraggableNodeRef, setDroppableNodeRef]
  )

  return children({
    setNodeRef,
    attributes: draggable.attributes,
    listeners: draggable.listeners,
    isDragging: draggable.isDragging,
    isOver: droppable.isOver,
  })
}

type TreeDropBindingsProps = {
  dropId: string
  dropData: TreeDropData
  disabled?: boolean
  children: (bindings: {
    setNodeRef: (node: HTMLElement | null) => void
    isOver: boolean
  }) => ReactNode
}

export function TreeDropBindings({
  dropId,
  dropData,
  disabled = false,
  children,
}: TreeDropBindingsProps) {
  const droppable = useDroppable({
    id: dropId,
    data: dropData as Data,
    disabled,
  })
  return children({
    setNodeRef: droppable.setNodeRef,
    isOver: droppable.isOver,
  })
}
