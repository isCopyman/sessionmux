"use client"

import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import { createPortal } from "react-dom"
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  pointerWithin,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragMoveEvent,
  type DragOverEvent,
  type DragStartEvent,
} from "@dnd-kit/core"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import {
  Archive,
  CheckSquare,
  ChevronDown,
  ChevronRight,
  Circle,
  Folder,
  FolderInput,
  FolderOpen,
  FolderPlus,
  FolderTree,
  Inbox,
  Info,
  Loader2,
  MoreHorizontal,
  PanelsTopLeft,
  Pencil,
  PanelBottomOpen,
  PanelRightOpen,
  Pin,
  PinOff,
  Plus,
  Square,
  SquarePen,
  Trash2,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { AgentIcon } from "@/components/agent-icon"
import { ConversationStatusDot } from "@/components/conversations/conversation-status-dot"
import { SessionBulkActionBar } from "@/components/conversations/session-bulk-action-bar"
import { SessionDetailsDialog } from "@/components/conversations/session-details-dialog"
import {
  TreeDndBindings,
  TreeDropBindings,
  canDropSessionOnTarget,
  collectionPlacementForRoot,
  collectionPlacementForRow,
  sessionDragPayload,
  sessionIdsInDrag,
  type CollectionDropTarget,
  type CollectionTreeDrag,
  type SessionTreeDrag,
  type TreeDragData,
  type TreeDropData,
} from "@/components/collections/collection-tree-dnd"

import { useImeGuard } from "@/hooks/use-ime-guard"
import { useSessionMultiSelect } from "@/hooks/use-session-multi-select"
import {
  assignConversationsToCollection,
  deleteConversation,
  listConversationCollectionRefs,
  updateConversationArchive,
  updateConversationPinned,
  updateConversationStatus,
  updateConversationTitle,
} from "@/lib/api"
import { visibleCollectionSessionIds } from "@/lib/collection-session-order"
import { formatConversationTitle } from "@/lib/conversation-title"
import { sessionClickIntent } from "@/lib/session-multi-select"
import {
  archiveSessions,
  deleteSessions,
  moveSessionsToCollection,
} from "@/lib/session-bulk-operations"
import type {
  AgentType,
  CollectionInfo,
  ConversationStatus,
  DbConversationSummary,
} from "@/lib/types"
import { STATUS_ORDER } from "@/lib/types"
import { toErrorMessage } from "@/lib/app-error"
import { cn } from "@/lib/utils"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useCollectionStore } from "@/stores/collection-store"
import { useOrganizationRevisionStore } from "@/stores/organization-revision-store"
import type { SidebarSortMode } from "@/lib/sidebar-view-mode-storage"
import { useTabActions, useTabStore } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"

type OpenScope = number | "unclassified"
type TreeDropIntent =
  | {
      kind: "session"
      rootFolderId: number
      collectionId: number | null
    }
  | { kind: "collection"; placement: CollectionDropTarget }

type EditorState =
  | {
      mode: "create"
      parentId: number | null
      rootFolderId?: number | null
    }
  | { mode: "rename"; item: CollectionInfo }
  | { mode: "move"; item: CollectionInfo }

interface CollectionTreeProps {
  onOpenScope: (scope: OpenScope) => void
  /** Render each Collection's Sessions inline instead of acting as a compact
   * Session Center shortcut tree. */
  showSessions?: boolean
  showCompleted?: boolean
  sortMode?: SidebarSortMode
  refreshKey?: number
  onOpenSession?: (session: DbConversationSummary) => void
  onOpenSessionInSplit?: (
    session: DbConversationSummary,
    direction: "right" | "down"
  ) => void
  /** Start a Session in the chosen canonical Path. */
  onNewSession?: (rootFolderId: number) => void
}

export interface CollectionTreeHandle {
  scrollToActive: () => void
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

function orderedChildren(items: CollectionInfo[]) {
  const result = new Map<number | null, CollectionInfo[]>()
  for (const item of items) {
    const siblings = result.get(item.parent_id) ?? []
    siblings.push(item)
    result.set(item.parent_id, siblings)
  }
  for (const siblings of result.values()) {
    siblings.sort((a, b) => a.position - b.position || a.id - b.id)
  }
  return result
}

function flatOptions(items: CollectionInfo[]) {
  const children = orderedChildren(items)
  const result: Array<{ item: CollectionInfo; depth: number }> = []
  const seen = new Set<number>()
  const append = (item: CollectionInfo, depth: number) => {
    if (seen.has(item.id)) return
    seen.add(item.id)
    result.push({ item, depth })
    for (const child of children.get(item.id) ?? []) append(child, depth + 1)
  }
  for (const root of children.get(null) ?? []) append(root, 0)
  for (const item of items) if (!seen.has(item.id)) append(item, 0)
  return result
}

export const CollectionTree = forwardRef<
  CollectionTreeHandle,
  CollectionTreeProps
>(function CollectionTree(
  {
    onOpenScope,
    showSessions = false,
    showCompleted = false,
    sortMode = "created",
    refreshKey = 0,
    onOpenSession,
    onOpenSessionInSplit,
    onNewSession,
  },
  ref
) {
  const t = useTranslations("Folder.sidebar.collections")
  const tSidebar = useTranslations("Folder.sidebar")
  const tCommon = useTranslations("Folder.common")
  const tConversation = useTranslations("Folder.conversationCard")
  const tManage = useTranslations("Folder.sidebar.manageConversations")
  const tStatus = useTranslations("Folder.statusLabels")
  const tDetails = useTranslations("Folder.sessionDetails")
  const ime = useImeGuard()
  const items = useCollectionStore((state) => state.items)
  const hydrated = useCollectionStore((state) => state.hydrated)
  const loading = useCollectionStore((state) => state.loading)
  const hydrate = useCollectionStore((state) => state.hydrate)
  const create = useCollectionStore((state) => state.create)
  const rename = useCollectionStore((state) => state.rename)
  const move = useCollectionStore((state) => state.move)
  const place = useCollectionStore((state) => state.place)
  const remove = useCollectionStore((state) => state.remove)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const folders = useAppWorkspaceStore((state) => state.folders)
  const allFolders = useAppWorkspaceStore((state) => state.allFolders)
  const activeFolderId = useAppWorkspaceStore((state) => state.activeFolderId)
  const updateConversationLocal = useAppWorkspaceStore(
    (state) => state.updateConversationLocal
  )
  const applyConversationUpsert = useAppWorkspaceStore(
    (state) => state.applyConversationUpsert
  )
  const applyConversationRemove = useAppWorkspaceStore(
    (state) => state.applyConversationRemove
  )
  const activeTabId = useTabStore((state) => state.activeTabId)
  const tabs = useTabStore((state) => state.tabs)
  const { closeConversationTab, openTab } = useTabActions()
  const { openConversations } = useWorkbenchRoute()
  const multiSelect = useSessionMultiSelect<DbConversationSummary>()
  const organizationRevision = useOrganizationRevisionStore(
    (state) => state.revision
  )

  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const [collapsedPaths, setCollapsedPaths] = useState<Set<number>>(new Set())
  const [collapsedUnclassified, setCollapsedUnclassified] = useState<
    Set<number>
  >(new Set())
  const [membershipByConversation, setMembershipByConversation] = useState<
    Map<number, number>
  >(new Map())
  const [membershipsHydrated, setMembershipsHydrated] = useState(false)
  const [membershipsLoading, setMembershipsLoading] = useState(false)
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [name, setName] = useState("")
  const [parentId, setParentId] = useState<number | null>(null)
  const [rootFolderId, setRootFolderId] = useState<number | null>(null)
  const [pending, setPending] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<CollectionInfo | null>(null)
  const [activeTreeDrag, setActiveTreeDrag] = useState<TreeDragData | null>(
    null
  )
  const [dropTarget, setDropTarget] = useState<string | null>(null)
  const [movingConversationId, setMovingConversationId] = useState<
    number | null
  >(null)
  const [collectionDropTarget, setCollectionDropTarget] =
    useState<CollectionDropTarget | null>(null)
  const [placingCollectionId, setPlacingCollectionId] = useState<number | null>(
    null
  )
  const [sessionRename, setSessionRename] = useState<{
    conversation: DbConversationSummary
    value: string
  } | null>(null)
  const [sessionDelete, setSessionDelete] =
    useState<DbConversationSummary | null>(null)
  const [sessionDetails, setSessionDetails] =
    useState<DbConversationSummary | null>(null)
  const [bulkDeleteOpen, setBulkDeleteOpen] = useState(false)
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const pendingLocateActiveRef = useRef(false)
  const [locateRequest, setLocateRequest] = useState(0)
  const dndSensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor)
  )

  const activeConversationId = useMemo(
    () => tabs.find((tab) => tab.id === activeTabId)?.conversationId ?? null,
    [activeTabId, tabs]
  )

  useEffect(() => {
    if (!hydrated) {
      void hydrate().catch((error) => {
        toast.error(t("operationFailed", { message: toErrorMessage(error) }))
      })
    }
  }, [hydrate, hydrated, t])

  const conversationIdsKey = useMemo(
    () =>
      conversations
        .filter((conversation) => conversation.archived_at == null)
        .map((conversation) => conversation.id)
        .sort((a, b) => a - b)
        .join(","),
    [conversations]
  )

  useEffect(() => {
    if (!showSessions) return
    const conversationIds = conversationIdsKey
      ? conversationIdsKey.split(",").map(Number)
      : []
    if (conversationIds.length === 0) {
      setMembershipByConversation(new Map())
      setMembershipsHydrated(true)
      return
    }
    let cancelled = false
    setMembershipsLoading(true)
    listConversationCollectionRefs(conversationIds)
      .then((refs) => {
        if (cancelled) return
        setMembershipByConversation(
          new Map(refs.map((ref) => [ref.conversation_id, ref.collection_id]))
        )
        setMembershipsHydrated(true)
      })
      .catch((error) => {
        if (!cancelled) {
          toast.error(t("operationFailed", { message: toErrorMessage(error) }))
        }
      })
      .finally(() => {
        if (!cancelled) setMembershipsLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [conversationIdsKey, organizationRevision, refreshKey, showSessions, t])

  const children = useMemo(() => orderedChildren(items), [items])
  const collectionById = useMemo(
    () => new Map(items.map((item) => [item.id, item])),
    [items]
  )
  const options = useMemo(() => flatOptions(items), [items])
  const invalidMoveParents = useMemo(
    () =>
      editor?.mode === "move"
        ? descendants(items, editor.item.id)
        : new Set<number>(),
    [editor, items]
  )
  const visibleConversations = useMemo(() => {
    if (showSessions && !membershipsHydrated) return []
    const next = conversations.filter(
      (conversation) =>
        conversation.archived_at == null &&
        (showCompleted || conversation.status !== "completed")
    )
    return next.sort((a, b) => {
      const aTime = Date.parse(
        sortMode === "updated" ? a.updated_at : a.created_at
      )
      const bTime = Date.parse(
        sortMode === "updated" ? b.updated_at : b.created_at
      )
      return bTime - aTime || b.id - a.id
    })
  }, [
    conversations,
    membershipsHydrated,
    showCompleted,
    showSessions,
    sortMode,
  ])
  const conversationsByCollection = useMemo(() => {
    const grouped = new Map<number, DbConversationSummary[]>()
    for (const conversation of visibleConversations) {
      const collectionId = membershipByConversation.get(conversation.id)
      if (collectionId == null) continue
      const group = grouped.get(collectionId) ?? []
      group.push(conversation)
      grouped.set(collectionId, group)
    }
    return grouped
  }, [membershipByConversation, visibleConversations])

  const folderById = useMemo(
    () => new Map(allFolders.map((folder) => [folder.id, folder])),
    [allFolders]
  )

  /** Canonical execution roots in the same order as the location sidebar.
   * Worktree folders contribute their parent Path instead of another root. */
  const pathRoots = useMemo(() => {
    const result: typeof allFolders = []
    const seen = new Set<number>()
    for (const folder of folders) {
      if (folder.kind === "chat") continue
      const rootId = folder.parent_id ?? folder.id
      if (seen.has(rootId)) continue
      const root = folderById.get(rootId)
      if (!root) continue
      seen.add(rootId)
      result.push(root)
    }
    return result
  }, [folderById, folders])

  const unclassifiedByRoot = useMemo(() => {
    const grouped = new Map<number, DbConversationSummary[]>()
    for (const conversation of visibleConversations) {
      if (membershipByConversation.has(conversation.id)) continue
      const folder = folderById.get(conversation.folder_id)
      if (!folder || folder.kind === "chat") continue
      const rootId = folder.parent_id ?? folder.id
      const group = grouped.get(rootId) ?? []
      group.push(conversation)
      grouped.set(rootId, group)
    }
    return grouped
  }, [folderById, membershipByConversation, visibleConversations])

  const conversationById = useMemo(
    () =>
      new Map(
        visibleConversations.map((conversation) => [
          conversation.id,
          conversation,
        ])
      ),
    [visibleConversations]
  )
  const rootFolderIdByConversation = useMemo(() => {
    const next = new Map<number, number>()
    for (const conversation of visibleConversations) {
      const folder = folderById.get(conversation.folder_id)
      if (!folder) continue
      next.set(conversation.id, folder.parent_id ?? folder.id)
    }
    return next
  }, [folderById, visibleConversations])

  const visibleSessionIds = useMemo(
    () =>
      visibleCollectionSessionIds({
        pathRoots,
        childrenByParent: children,
        conversationsByCollection,
        unclassifiedByRoot,
        expanded,
        collapsedPaths,
        collapsedUnclassified,
      }),
    [
      children,
      collapsedPaths,
      collapsedUnclassified,
      conversationsByCollection,
      expanded,
      pathRoots,
      unclassifiedByRoot,
    ]
  )

  const sessionCountByRoot = useMemo(() => {
    const result = new Map<number, number>()
    for (const conversation of visibleConversations) {
      const collectionId = membershipByConversation.get(conversation.id)
      const collection =
        collectionId == null ? undefined : collectionById.get(collectionId)
      const folder = folderById.get(conversation.folder_id)
      const rootId =
        collection?.root_folder_id ??
        (folder ? (folder.parent_id ?? folder.id) : null)
      if (rootId == null) continue
      result.set(rootId, (result.get(rootId) ?? 0) + 1)
    }
    return result
  }, [
    collectionById,
    folderById,
    membershipByConversation,
    visibleConversations,
  ])

  const canonicalActiveRootId = useMemo(() => {
    const active = allFolders.find((folder) => folder.id === activeFolderId)
    return active ? (active.parent_id ?? active.id) : null
  }, [activeFolderId, allFolders])

  useImperativeHandle(ref, () => ({
    scrollToActive() {
      if (activeConversationId == null) return
      const conversation = visibleConversations.find(
        (candidate) => candidate.id === activeConversationId
      )
      if (!conversation) return

      const collectionId = membershipByConversation.get(conversation.id)
      const collection =
        collectionId == null ? undefined : collectionById.get(collectionId)
      const folder = folderById.get(conversation.folder_id)
      const rootId =
        collection?.root_folder_id ??
        (folder ? (folder.parent_id ?? folder.id) : null)

      if (rootId != null) {
        setCollapsedPaths((current) => {
          if (!current.has(rootId)) return current
          const next = new Set(current)
          next.delete(rootId)
          return next
        })
      }

      if (collectionId == null) {
        if (rootId != null) {
          setCollapsedUnclassified((current) => {
            if (!current.has(rootId)) return current
            const next = new Set(current)
            next.delete(rootId)
            return next
          })
        }
      } else {
        setExpanded((current) => {
          const next = new Set(current)
          let cursor: number | null = collectionId
          while (cursor != null) {
            next.add(cursor)
            cursor = collectionById.get(cursor)?.parent_id ?? null
          }
          return next
        })
      }

      pendingLocateActiveRef.current = true
      setLocateRequest((current) => current + 1)
    },
  }))

  useEffect(() => {
    if (!pendingLocateActiveRef.current || activeConversationId == null) return
    const row = scrollContainerRef.current?.querySelector<HTMLElement>(
      `[data-conversation-id="${activeConversationId}"]`
    )
    if (!row) return
    pendingLocateActiveRef.current = false
    row.scrollIntoView({ behavior: "smooth", block: "center" })
  }, [
    activeConversationId,
    collapsedPaths,
    collapsedUnclassified,
    expanded,
    membershipByConversation,
    locateRequest,
    visibleConversations,
  ])

  const sessionRootId = (conversation: DbConversationSummary) => {
    const folder = folderById.get(conversation.folder_id)
    return folder ? (folder.parent_id ?? folder.id) : null
  }

  const dragTargetKey = (rootId: number, collectionId: number | null) =>
    `${rootId}:${collectionId ?? "unclassified"}`

  const canDropSession = (
    payload: SessionTreeDrag,
    rootId: number,
    collectionId: number | null
  ) => {
    return canDropSessionOnTarget(
      payload,
      rootId,
      collectionId,
      membershipByConversation.get(payload.conversationId) ?? null,
      movingConversationId != null,
      membershipByConversation
    )
  }

  const applySessionMove = async (
    payload: SessionTreeDrag,
    rootId: number,
    collectionId: number | null
  ) => {
    if (!canDropSession(payload, rootId, collectionId)) return

    const movingIds = sessionIdsInDrag(payload).filter(
      (id) => (membershipByConversation.get(id) ?? null) !== collectionId
    )
    if (movingIds.length === 0) return

    setDropTarget(null)
    setMovingConversationId(payload.conversationId)
    try {
      await assignConversationsToCollection(movingIds, collectionId)
      setMembershipByConversation((current) => {
        const next = new Map(current)
        for (const id of movingIds) {
          if (collectionId == null) next.delete(id)
          else next.set(id, collectionId)
        }
        return next
      })
      if (collectionId == null) {
        setCollapsedUnclassified((current) => {
          const next = new Set(current)
          next.delete(rootId)
          return next
        })
      } else {
        setExpanded((current) => new Set(current).add(collectionId))
      }
      if (movingIds.length > 1) multiSelect.clear()
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    } finally {
      setMovingConversationId(null)
    }
  }

  const collectionPlacementAt = (
    payload: CollectionTreeDrag,
    target: CollectionInfo,
    rect: Pick<DOMRect, "top" | "height">,
    clientY: number
  ): CollectionDropTarget | null => {
    return collectionPlacementForRow(
      payload,
      target,
      rect,
      clientY,
      items,
      placingCollectionId != null
    )
  }

  const collectionRootPlacement = (
    payload: CollectionTreeDrag,
    rootId: number | null
  ): CollectionDropTarget | null => {
    return collectionPlacementForRoot(
      payload,
      rootId,
      items,
      placingCollectionId != null
    )
  }

  const applyCollectionPlace = async (
    payload: CollectionTreeDrag,
    placement: CollectionDropTarget
  ) => {
    setCollectionDropTarget(null)
    setPlacingCollectionId(payload.collectionId)
    try {
      await place(payload.collectionId, placement.parentId, placement.index)
      if (placement.parentId != null) {
        setExpanded((current) => new Set(current).add(placement.parentId!))
      }
      if (typeof placement.rootFolderId === "number") {
        setCollapsedPaths((current) => {
          const next = new Set(current)
          next.delete(placement.rootFolderId as number)
          return next
        })
      }
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    } finally {
      setPlacingCollectionId(null)
    }
  }

  const pointerYForDrag = (
    event: DragMoveEvent | DragOverEvent | DragEndEvent
  ) => {
    const activator = event.activatorEvent
    if ("clientY" in activator && typeof activator.clientY === "number") {
      return activator.clientY + event.delta.y
    }
    const translated = event.active.rect.current.translated
    return translated
      ? translated.top + translated.height / 2
      : (event.over?.rect.top ?? 0) + (event.over?.rect.height ?? 0) / 2
  }

  const resolveTreeDropIntent = (
    event: DragMoveEvent | DragOverEvent | DragEndEvent
  ): TreeDropIntent | null => {
    const dragged = event.active.data.current as TreeDragData | undefined
    const target = event.over?.data.current as TreeDropData | undefined
    if (!dragged || !target) return null

    if (dragged.kind === "session") {
      const destination =
        target.kind === "session-bucket"
          ? target
          : target.kind === "collection-row"
            ? {
                rootFolderId: target.rootFolderId,
                collectionId: target.collectionId,
              }
            : null
      if (
        !destination ||
        typeof destination.rootFolderId !== "number" ||
        !canDropSession(
          dragged,
          destination.rootFolderId,
          destination.collectionId
        )
      ) {
        return null
      }
      return {
        kind: "session",
        rootFolderId: destination.rootFolderId,
        collectionId: destination.collectionId,
      }
    }

    if (target.kind === "collection-row") {
      const item = collectionById.get(target.collectionId)
      if (!item) return null
      const placement = collectionPlacementAt(
        dragged,
        item,
        event.over!.rect,
        pointerYForDrag(event)
      )
      return placement ? { kind: "collection", placement } : null
    }
    if (target.kind === "collection-root") {
      const placement = collectionRootPlacement(dragged, target.rootFolderId)
      return placement ? { kind: "collection", placement } : null
    }
    return null
  }

  const showTreeDropIntent = (intent: TreeDropIntent | null) => {
    if (intent?.kind === "session") {
      setDropTarget(dragTargetKey(intent.rootFolderId, intent.collectionId))
      setCollectionDropTarget(null)
      return
    }
    setDropTarget(null)
    setCollectionDropTarget(
      intent?.kind === "collection" ? intent.placement : null
    )
  }

  const handleTreeDragStart = (event: DragStartEvent) => {
    const dragged = event.active.data.current as TreeDragData | undefined
    if (!dragged) return
    setActiveTreeDrag(dragged)
    showTreeDropIntent(null)
  }

  const handleTreeDragMove = (event: DragMoveEvent | DragOverEvent) => {
    showTreeDropIntent(resolveTreeDropIntent(event))
  }

  const clearTreeDrag = () => {
    setActiveTreeDrag(null)
    showTreeDropIntent(null)
  }

  const handleTreeDragEnd = (event: DragEndEvent) => {
    const dragged = event.active.data.current as TreeDragData | undefined
    const intent = resolveTreeDropIntent(event)
    clearTreeDrag()
    if (!dragged || !intent) return
    if (dragged.kind === "session" && intent.kind === "session") {
      void applySessionMove(dragged, intent.rootFolderId, intent.collectionId)
      return
    }
    if (dragged.kind === "collection" && intent.kind === "collection") {
      void applyCollectionPlace(dragged, intent.placement)
    }
  }

  const openEditor = (next: EditorState) => {
    setEditor(next)
    if (next.mode === "create") {
      setName("")
      setParentId(next.parentId)
      const parent =
        next.parentId == null
          ? null
          : items.find((item) => item.id === next.parentId)
      setRootFolderId(
        parent?.root_folder_id ??
          next.rootFolderId ??
          canonicalActiveRootId ??
          allFolders.find(
            (folder) => folder.kind === "regular" && folder.parent_id == null
          )?.id ??
          null
      )
    } else {
      setName(next.item.name)
      setParentId(next.item.parent_id)
      setRootFolderId(next.item.root_folder_id ?? null)
    }
  }

  const submitEditor = async () => {
    if (!editor) return
    const normalizedName = name.trim()
    if (editor.mode !== "move" && !normalizedName) return
    setPending(true)
    try {
      if (editor.mode === "create") {
        const created = await create(normalizedName, parentId, rootFolderId)
        if (parentId != null) {
          setExpanded((current) => new Set(current).add(parentId))
        }
        if (!showSessions) onOpenScope(created.id)
      } else if (editor.mode === "rename") {
        await rename(editor.item.id, normalizedName)
      } else {
        await move(editor.item.id, parentId)
        if (parentId != null) {
          setExpanded((current) => new Set(current).add(parentId))
        }
      }
      setEditor(null)
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    } finally {
      setPending(false)
    }
  }

  const confirmDelete = async () => {
    if (!deleteTarget) return
    setPending(true)
    try {
      await remove(deleteTarget.id)
      setDeleteTarget(null)
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    } finally {
      setPending(false)
    }
  }

  const handleSessionRename = async () => {
    if (!sessionRename) return
    const trimmed = sessionRename.value.trim()
    if (!trimmed || trimmed === (sessionRename.conversation.title ?? "")) {
      setSessionRename(null)
      return
    }
    try {
      await updateConversationTitle(sessionRename.conversation.id, trimmed)
      updateConversationLocal(sessionRename.conversation.id, { title: trimmed })
      setSessionRename(null)
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    }
  }

  const handleSessionDelete = async () => {
    if (!sessionDelete) return
    try {
      await deleteConversation(sessionDelete.id)
      closeConversationTab(
        sessionDelete.folder_id,
        sessionDelete.id,
        sessionDelete.agent_type as AgentType
      )
      applyConversationRemove(sessionDelete.id)
      setSessionDelete(null)
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    }
  }

  const handleSessionArchive = async (conversation: DbConversationSummary) => {
    try {
      await updateConversationArchive(conversation.id, true)
      applyConversationUpsert({
        ...conversation,
        archived_at: new Date().toISOString(),
      })
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    }
  }

  const handleSessionPin = async (
    conversation: DbConversationSummary,
    nextPinned: boolean
  ) => {
    updateConversationLocal(conversation.id, {
      pinned_at: nextPinned ? new Date().toISOString() : null,
    })
    try {
      await updateConversationPinned(conversation.id, nextPinned)
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    }
  }

  const handleSessionStatus = async (
    conversation: DbConversationSummary,
    status: ConversationStatus
  ) => {
    updateConversationLocal(conversation.id, { status })
    try {
      await updateConversationStatus(conversation.id, status)
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    }
  }

  const selectedSessions = () => [...multiSelect.selected.values()]

  const handleBulkArchive = async () => {
    const conversations = selectedSessions()
    if (conversations.length === 0) return
    try {
      await archiveSessions(conversations)
      toast.success(tManage("toastArchived", { count: conversations.length }))
      multiSelect.clear()
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    }
  }

  const handleBulkMove = async (collectionId: number | null) => {
    const conversations = selectedSessions()
    if (conversations.length === 0) return
    try {
      await moveSessionsToCollection(
        conversations.map((conversation) => conversation.id),
        collectionId
      )
      toast.success(
        tManage("toastCollectionMoved", { count: conversations.length })
      )
      multiSelect.clear()
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    }
  }

  const handleBulkAddToCurrentWorkbench = () => {
    const conversations = selectedSessions()
    if (conversations.length === 0) return
    openConversations()
    for (const conversation of conversations) {
      openTab(
        conversation.folder_id,
        conversation.id,
        conversation.agent_type,
        true,
        formatConversationTitle(conversation.title)
      )
    }
    toast.success(tManage("toastOpened", { count: conversations.length }))
    multiSelect.clear()
  }

  const handleBulkDelete = async () => {
    const conversations = selectedSessions()
    if (conversations.length === 0) return
    try {
      await deleteSessions(conversations, closeConversationTab)
      toast.success(tManage("toastDeleted", { count: conversations.length }))
      setBulkDeleteOpen(false)
      multiSelect.clear()
    } catch (error) {
      toast.error(t("operationFailed", { message: toErrorMessage(error) }))
    }
  }

  const lookupVisibleSession = (id: number) => conversationById.get(id)

  const handleSessionActivate = (
    conversation: DbConversationSummary,
    event?: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean }
  ) => {
    const intent = sessionClickIntent(event)
    if (intent !== "open") {
      multiSelect.apply(
        conversation,
        intent,
        visibleSessionIds,
        lookupVisibleSession
      )
      return
    }
    if (multiSelect.selected.size > 0) multiSelect.clear()
    onOpenSession?.(conversation)
  }

  const handleSessionTreeKey = (
    event: { key: string; preventDefault: () => void },
    conversation: DbConversationSummary
  ) => {
    if (event.key === "F2") {
      event.preventDefault()
      setSessionRename({
        conversation,
        value: conversation.title ?? "",
      })
      return
    }
    if (event.key === "Delete") {
      event.preventDefault()
      if (
        multiSelect.selected.size > 1 &&
        multiSelect.selected.has(conversation.id)
      ) {
        setBulkDeleteOpen(true)
        return
      }
      setSessionDelete(conversation)
    }
  }

  const handleCollectionTreeKey = (
    event: { key: string; preventDefault: () => void },
    item: CollectionInfo
  ) => {
    if (event.key === "F2") {
      event.preventDefault()
      openEditor({ mode: "rename", item })
      return
    }
    if (event.key === "Delete") {
      event.preventDefault()
      setDeleteTarget(item)
    }
  }

  const renderSession = (
    conversation: DbConversationSummary,
    depth: number,
    collectionId: number | null
  ) => {
    const selected = conversation.id === activeConversationId
    const checked = multiSelect.selected.has(conversation.id)
    const multiSelectActive = multiSelect.selected.size > 0
    const bulkMenu =
      multiSelectActive && checked && multiSelect.selected.size > 1
    const rootId = sessionRootId(conversation)
    const dropKey = rootId == null ? null : dragTargetKey(rootId, collectionId)
    const isPinned = conversation.pinned_at != null
    const title =
      formatConversationTitle(conversation.title) ||
      tConversation("untitledConversation")
    const dragPayload =
      rootId == null
        ? null
        : sessionDragPayload({
            grabbedId: conversation.id,
            grabbedRootFolderId: rootId,
            grabbedLabel: title,
            selectedIds: [...multiSelect.selected.keys()],
            rootFolderIdByConversation,
          })
    const dragIds = dragPayload ? sessionIdsInDrag(dragPayload) : []
    const groupDragging =
      activeTreeDrag?.kind === "session" &&
      sessionIdsInDrag(activeTreeDrag).includes(conversation.id)
    const row = (
      <TreeDndBindings
        dragId={`session:${conversation.id}`}
        dragData={
          dragPayload ?? {
            kind: "session",
            conversationId: conversation.id,
            rootFolderId: -1,
            label: title,
          }
        }
        dropId={`session-bucket:${conversation.id}`}
        dropData={{
          kind: "session-bucket",
          rootFolderId: rootId ?? -1,
          collectionId,
        }}
        disabled={
          !showSessions || rootId == null || movingConversationId != null
        }
      >
        {({ setNodeRef, attributes, listeners, isDragging }) => (
          <div
            className={cn(
              "group flex min-w-0 items-center rounded-md hover:bg-sidebar-accent",
              selected &&
                "bg-primary/8 text-primary ring-1 ring-inset ring-primary/30",
              checked &&
                "bg-sidebar-primary/12 ring-1 ring-inset ring-primary/25",
              dropKey != null &&
                dropTarget === dropKey &&
                "bg-primary/10 ring-1 ring-inset ring-primary/45",
              (isDragging || groupDragging) && "opacity-45",
              (movingConversationId === conversation.id ||
                (movingConversationId != null &&
                  dragIds.includes(conversation.id))) &&
                "pointer-events-none opacity-55"
            )}
            style={{ paddingInlineStart: `${0.75 + depth * 0.75}rem` }}
          >
            <button
              type="button"
              tabIndex={-1}
              data-session-select={conversation.id}
              aria-pressed={checked}
              aria-label={tManage("selectConversation", { title })}
              className={cn(
                "flex h-4 w-0 shrink-0 items-center justify-center overflow-hidden rounded-sm text-muted-foreground hover:text-foreground",
                "opacity-0 pointer-events-none",
                "group-hover:h-4 group-hover:w-4 group-hover:opacity-100 group-hover:pointer-events-auto",
                (multiSelectActive || checked) &&
                  "h-4 w-4 opacity-100 pointer-events-auto"
              )}
              onPointerDown={(event) => event.stopPropagation()}
              onClick={(event) => {
                event.preventDefault()
                event.stopPropagation()
                multiSelect.apply(
                  conversation,
                  "toggle",
                  visibleSessionIds,
                  lookupVisibleSession
                )
              }}
            >
              {checked ? (
                <CheckSquare className="h-3.5 w-3.5 text-primary" />
              ) : (
                <Square className="h-3.5 w-3.5" />
              )}
            </button>
            <button
              ref={setNodeRef}
              type="button"
              {...attributes}
              {...listeners}
              data-conversation-id={conversation.id}
              data-session-root-id={rootId ?? undefined}
              data-session-collection-id={
                collectionId == null ? "" : String(collectionId)
              }
              data-focused-session={selected ? "true" : undefined}
              data-session-checked={checked ? "true" : undefined}
              data-session-drag-ids={
                dragIds.length > 0 ? dragIds.join(",") : undefined
              }
              data-session-dragging={isDragging ? "true" : undefined}
              data-session-drop-target={
                dropKey != null && dropTarget === dropKey ? "true" : undefined
              }
              aria-current={selected ? "page" : undefined}
              title={title}
              className={cn(
                "flex h-7 min-w-0 flex-1 cursor-grab touch-none items-center gap-1.5 pe-2 text-start text-xs active:cursor-grabbing focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                (multiSelectActive || checked) && "ps-1",
                "group-hover:ps-1"
              )}
              onClick={(event) => handleSessionActivate(conversation, event)}
              onKeyDown={(event) => {
                listeners?.onKeyDown?.(event)
                if (!event.defaultPrevented) {
                  handleSessionTreeKey(event, conversation)
                }
              }}
            >
              <span
                aria-hidden
                data-session-agent-icon=""
                className="relative flex h-3.5 w-3.5 shrink-0 items-center justify-center"
              >
                <AgentIcon
                  agentType={conversation.agent_type}
                  className="h-3 w-3"
                />
                <ConversationStatusDot
                  status={conversation.status as ConversationStatus}
                  size="sm"
                  className="absolute -bottom-0.5 -right-0.5 ring-1 ring-sidebar"
                />
              </span>
              <span className="min-w-0 flex-1 truncate">{title}</span>
            </button>
          </div>
        )}
      </TreeDndBindings>
    )
    return (
      <ContextMenu key={conversation.id}>
        <ContextMenuTrigger asChild>
          <div className="min-w-0">{row}</div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          {bulkMenu ? (
            <>
              <ContextMenuItem onSelect={() => void handleBulkArchive()}>
                <Archive className="h-4 w-4" />
                {tManage("archiveSelected")}
              </ContextMenuItem>
              <ContextMenuSub>
                <ContextMenuSubTrigger>
                  <FolderTree className="h-4 w-4" />
                  {tManage("moveToCollection")}
                </ContextMenuSubTrigger>
                <ContextMenuSubContent className="max-h-72 overflow-y-auto">
                  <ContextMenuItem onSelect={() => void handleBulkMove(null)}>
                    {tManage("collectionUnclassified")}
                  </ContextMenuItem>
                  {options.map(({ item, depth: optionDepth }) => (
                    <ContextMenuItem
                      key={item.id}
                      onSelect={() => void handleBulkMove(item.id)}
                    >
                      <span className="truncate">
                        {optionDepth > 0 ? `${"· ".repeat(optionDepth)}` : ""}
                        {item.name}
                      </span>
                    </ContextMenuItem>
                  ))}
                </ContextMenuSubContent>
              </ContextMenuSub>
              <ContextMenuItem onSelect={handleBulkAddToCurrentWorkbench}>
                <PanelsTopLeft className="h-4 w-4" />
                {tManage("addToWorkbench")}
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem
                variant="destructive"
                onSelect={() => setBulkDeleteOpen(true)}
              >
                <Trash2 className="h-4 w-4" />
                {tManage("deleteSelected")}
              </ContextMenuItem>
            </>
          ) : (
            <>
              {onOpenSessionInSplit ? (
                <>
                  <ContextMenuItem
                    onSelect={() => onOpenSessionInSplit(conversation, "right")}
                  >
                    <PanelRightOpen className="h-4 w-4" />
                    {tConversation("openRight")}
                  </ContextMenuItem>
                  <ContextMenuItem
                    onSelect={() => onOpenSessionInSplit(conversation, "down")}
                  >
                    <PanelBottomOpen className="h-4 w-4" />
                    {tConversation("openDown")}
                  </ContextMenuItem>
                  <ContextMenuSeparator />
                </>
              ) : null}
              {onNewSession && rootId != null ? (
                <>
                  <ContextMenuItem onSelect={() => onNewSession(rootId)}>
                    <SquarePen className="h-4 w-4" />
                    {tConversation("newConversation")}
                  </ContextMenuItem>
                  <ContextMenuSeparator />
                </>
              ) : null}
              <ContextMenuItem
                onSelect={() =>
                  setSessionRename({
                    conversation,
                    value: conversation.title ?? "",
                  })
                }
              >
                <Pencil className="h-4 w-4" />
                {tConversation("rename")}
                <span className="ms-auto text-[10px] text-muted-foreground">
                  F2
                </span>
              </ContextMenuItem>
              <ContextMenuItem
                onSelect={() => void handleSessionPin(conversation, !isPinned)}
              >
                {isPinned ? (
                  <PinOff className="h-4 w-4" />
                ) : (
                  <Pin className="h-4 w-4" />
                )}
                {isPinned ? tConversation("unpin") : tConversation("pin")}
              </ContextMenuItem>
              <ContextMenuItem
                onSelect={() => void handleSessionArchive(conversation)}
              >
                <Archive className="h-4 w-4" />
                {tConversation("archive")}
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => setSessionDetails(conversation)}>
                <Info className="h-4 w-4" />
                {tDetails("menuLabel")}
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuSub>
                <ContextMenuSubTrigger>
                  <Circle className="h-4 w-4" />
                  {tConversation("status")}
                </ContextMenuSubTrigger>
                <ContextMenuSubContent>
                  {STATUS_ORDER.filter(
                    (status) => status !== conversation.status
                  ).map((status) => (
                    <ContextMenuItem
                      key={status}
                      onSelect={() =>
                        void handleSessionStatus(conversation, status)
                      }
                    >
                      <ConversationStatusDot status={status} />
                      {tStatus(status)}
                    </ContextMenuItem>
                  ))}
                </ContextMenuSubContent>
              </ContextMenuSub>
              <ContextMenuSeparator />
              <ContextMenuItem
                variant="destructive"
                onSelect={() => setSessionDelete(conversation)}
              >
                <Trash2 className="h-4 w-4" />
                {tConversation("delete")}
                <span className="ms-auto text-[10px] text-muted-foreground">
                  Del
                </span>
              </ContextMenuItem>
            </>
          )}
        </ContextMenuContent>
      </ContextMenu>
    )
  }

  const renderItems = (
    parent: number | null,
    depth = 0,
    rootFolderId?: number | null
  ): ReactNode =>
    (children.get(parent) ?? [])
      .filter(
        (item) =>
          rootFolderId === undefined ||
          (item.root_folder_id ?? null) === rootFolderId
      )
      .map((item) => {
        const childItems = children.get(item.id) ?? []
        const memberSessions = showSessions
          ? (conversationsByCollection.get(item.id) ?? [])
          : []
        const expandable = childItems.length > 0 || memberSessions.length > 0
        const isExpanded = expanded.has(item.id)
        return (
          <TreeDndBindings
            key={item.id}
            dragId={`collection:${item.id}`}
            dragData={{
              kind: "collection",
              collectionId: item.id,
              rootFolderId: item.root_folder_id ?? null,
              label: item.name,
            }}
            dropId={`collection-row:${item.id}`}
            dropData={{
              kind: "collection-row",
              collectionId: item.id,
              rootFolderId: item.root_folder_id ?? null,
            }}
            disabled={placingCollectionId != null}
          >
            {({ setNodeRef, attributes, listeners, isDragging }) => (
              <div>
                <div
                  ref={setNodeRef}
                  data-collection-id={item.id}
                  data-collection-root-id={item.root_folder_id ?? undefined}
                  data-collection-drop-position={
                    collectionDropTarget?.targetCollectionId === item.id
                      ? collectionDropTarget.position
                      : undefined
                  }
                  data-session-drop-target={
                    dropTarget ===
                    dragTargetKey(item.root_folder_id ?? -1, item.id)
                      ? "true"
                      : undefined
                  }
                  className={cn(
                    "group relative flex h-7 min-w-0 items-center rounded-md pe-1 hover:bg-sidebar-accent",
                    dropTarget ===
                      dragTargetKey(item.root_folder_id ?? -1, item.id) &&
                      "bg-primary/10 ring-1 ring-inset ring-primary/45",
                    activeTreeDrag?.kind === "session" &&
                      canDropSession(
                        activeTreeDrag,
                        item.root_folder_id ?? -1,
                        item.id
                      ) &&
                      "ring-1 ring-dashed ring-primary/30",
                    collectionDropTarget?.targetCollectionId === item.id &&
                      collectionDropTarget.position === "inside" &&
                      "bg-primary/10 ring-1 ring-inset ring-primary/45",
                    isDragging && "opacity-45",
                    placingCollectionId === item.id &&
                      "pointer-events-none opacity-55"
                  )}
                  style={{ paddingInlineStart: `${0.25 + depth * 0.75}rem` }}
                >
                  {collectionDropTarget?.targetCollectionId === item.id &&
                  (collectionDropTarget.position === "before" ||
                    collectionDropTarget.position === "after") ? (
                    <span
                      aria-hidden
                      className={cn(
                        "pointer-events-none absolute inset-x-1 z-10 h-0.5 rounded-full bg-primary",
                        collectionDropTarget.position === "before"
                          ? "-top-px"
                          : "-bottom-px"
                      )}
                    />
                  ) : null}
                  <button
                    type="button"
                    className={cn(
                      "flex h-6 w-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground",
                      !expandable && "pointer-events-none opacity-0"
                    )}
                    aria-label={isExpanded ? t("collapse") : t("expand")}
                    onClick={() =>
                      setExpanded((current) => {
                        const next = new Set(current)
                        if (next.has(item.id)) next.delete(item.id)
                        else next.add(item.id)
                        return next
                      })
                    }
                  >
                    {isExpanded ? (
                      <ChevronDown className="h-3.5 w-3.5" />
                    ) : (
                      <ChevronRight className="h-3.5 w-3.5 rtl:rotate-180" />
                    )}
                  </button>
                  <ContextMenu>
                    <ContextMenuTrigger asChild>
                      <div className="flex min-h-0 min-w-0 flex-1">
                        <button
                          type="button"
                          {...attributes}
                          {...listeners}
                          data-collection-dragging={
                            isDragging ? "true" : undefined
                          }
                          className="flex h-full min-w-0 flex-1 cursor-grab touch-none items-center gap-1.5 text-start text-xs active:cursor-grabbing"
                          title={item.name}
                          aria-label={item.name}
                          onKeyDown={(event) => {
                            listeners?.onKeyDown?.(event)
                            if (!event.defaultPrevented) {
                              handleCollectionTreeKey(event, item)
                            }
                          }}
                          onClick={() => {
                            if (!showSessions) {
                              onOpenScope(item.id)
                              return
                            }
                            setExpanded((current) => {
                              const next = new Set(current)
                              if (next.has(item.id)) next.delete(item.id)
                              else next.add(item.id)
                              return next
                            })
                          }}
                        >
                          {isExpanded ? (
                            <FolderOpen className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                          ) : (
                            <Folder className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                          )}
                          <span className="truncate">{item.name}</span>
                          {showSessions && memberSessions.length > 0 ? (
                            <span className="ms-auto shrink-0 text-[10px] text-muted-foreground">
                              {memberSessions.length}
                            </span>
                          ) : null}
                        </button>
                      </div>
                    </ContextMenuTrigger>
                    <ContextMenuContent>
                      <ContextMenuItem onSelect={() => onOpenScope(item.id)}>
                        <FolderOpen className="h-4 w-4" />
                        {t("openInSessionCenter")}
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() =>
                          openEditor({ mode: "create", parentId: item.id })
                        }
                      >
                        <FolderPlus className="h-4 w-4" />
                        {t("newChild")}
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() => openEditor({ mode: "rename", item })}
                      >
                        <Pencil className="h-4 w-4" />
                        {t("rename")}
                        <span className="ms-auto text-[10px] text-muted-foreground">
                          F2
                        </span>
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() => openEditor({ mode: "move", item })}
                      >
                        <FolderInput className="h-4 w-4" />
                        {t("move")}
                      </ContextMenuItem>
                      <ContextMenuSeparator />
                      <ContextMenuItem
                        variant="destructive"
                        onSelect={() => setDeleteTarget(item)}
                      >
                        <Trash2 className="h-4 w-4" />
                        {t("delete")}
                        <span className="ms-auto text-[10px] text-muted-foreground">
                          Del
                        </span>
                      </ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        size="icon-sm"
                        variant="ghost"
                        className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
                        aria-label={t("actions", { name: item.name })}
                      >
                        <MoreHorizontal className="h-3.5 w-3.5" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onSelect={() => onOpenScope(item.id)}>
                        <FolderOpen className="h-4 w-4" />
                        {t("openInSessionCenter")}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={() =>
                          openEditor({ mode: "create", parentId: item.id })
                        }
                      >
                        <FolderPlus className="h-4 w-4" />
                        {t("newChild")}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={() => openEditor({ mode: "rename", item })}
                      >
                        <Pencil className="h-4 w-4" />
                        {t("rename")}
                        <span className="ms-auto text-[10px] text-muted-foreground">
                          F2
                        </span>
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={() => openEditor({ mode: "move", item })}
                      >
                        <FolderInput className="h-4 w-4" />
                        {t("move")}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        variant="destructive"
                        onSelect={() => setDeleteTarget(item)}
                      >
                        <Trash2 className="h-4 w-4" />
                        {t("delete")}
                        <span className="ms-auto text-[10px] text-muted-foreground">
                          Del
                        </span>
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
                {isExpanded ? (
                  <>
                    {memberSessions.map((session) =>
                      renderSession(session, depth + 1, item.id)
                    )}
                    {renderItems(item.id, depth + 1, rootFolderId)}
                  </>
                ) : null}
              </div>
            )}
          </TreeDndBindings>
        )
      })

  const renderUnclassified = (
    rootFolderId: number,
    sessions: DbConversationSummary[]
  ) => {
    const isExpanded = !collapsedUnclassified.has(rootFolderId)
    return (
      <TreeDropBindings
        key={`unclassified:${rootFolderId}`}
        dropId={`unclassified:${rootFolderId}`}
        dropData={{
          kind: "session-bucket",
          rootFolderId,
          collectionId: null,
        }}
        disabled={false}
      >
        {({ setNodeRef }) => (
          <div>
            <div
              ref={setNodeRef}
              data-unclassified-root-id={rootFolderId}
              data-session-drop-target={
                dropTarget === dragTargetKey(rootFolderId, null)
                  ? "true"
                  : undefined
              }
              className={cn(
                "group flex h-7 min-w-0 items-center rounded-md pe-1 ps-4 hover:bg-sidebar-accent",
                dropTarget === dragTargetKey(rootFolderId, null) &&
                  "bg-primary/10 ring-1 ring-inset ring-primary/45",
                activeTreeDrag?.kind === "session" &&
                  canDropSession(activeTreeDrag, rootFolderId, null) &&
                  "ring-1 ring-dashed ring-primary/30"
              )}
            >
              <button
                type="button"
                className={cn(
                  "flex h-6 w-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground",
                  sessions.length === 0 && "pointer-events-none opacity-0"
                )}
                aria-label={isExpanded ? t("collapse") : t("expand")}
                onClick={() =>
                  setCollapsedUnclassified((current) => {
                    const next = new Set(current)
                    if (next.has(rootFolderId)) next.delete(rootFolderId)
                    else next.add(rootFolderId)
                    return next
                  })
                }
              >
                {isExpanded ? (
                  <ChevronDown className="h-3.5 w-3.5" />
                ) : (
                  <ChevronRight className="h-3.5 w-3.5 rtl:rotate-180" />
                )}
              </button>
              <button
                type="button"
                className="flex h-full min-w-0 flex-1 items-center gap-1.5 text-start text-xs"
                aria-label={t("unclassified")}
                onClick={() => {
                  if (sessions.length === 0) return
                  setCollapsedUnclassified((current) => {
                    const next = new Set(current)
                    if (next.has(rootFolderId)) next.delete(rootFolderId)
                    else next.add(rootFolderId)
                    return next
                  })
                }}
              >
                <Inbox className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate">{t("unclassified")}</span>
                {sessions.length > 0 ? (
                  <span className="ms-auto shrink-0 text-[10px] text-muted-foreground">
                    {sessions.length}
                  </span>
                ) : null}
              </button>
              <Button
                size="icon-sm"
                variant="ghost"
                className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                aria-label={t("openInSessionCenter")}
                onClick={() => onOpenScope("unclassified")}
              >
                <MoreHorizontal className="h-3.5 w-3.5" />
              </Button>
            </div>
            {isExpanded
              ? sessions.map((session) => renderSession(session, 1, null))
              : null}
          </div>
        )}
      </TreeDropBindings>
    )
  }

  const renderPath = (root: (typeof allFolders)[number]) => {
    const isExpanded = !collapsedPaths.has(root.id)
    const sessions = unclassifiedByRoot.get(root.id) ?? []
    const rootCollections = (children.get(null) ?? []).filter(
      (item) => item.root_folder_id === root.id
    )
    const expandable = rootCollections.length > 0 || sessions.length > 0
    return (
      <TreeDropBindings
        key={root.id}
        dropId={`collection-root:${root.id}`}
        dropData={{ kind: "collection-root", rootFolderId: root.id }}
        disabled={false}
      >
        {({ setNodeRef }) => (
          <div data-collection-path={root.id}>
            <div
              ref={setNodeRef}
              data-collection-root-drop={
                collectionDropTarget?.position === "root" &&
                collectionDropTarget.rootFolderId === root.id
                  ? "true"
                  : undefined
              }
              className={cn(
                "group flex h-8 min-w-0 items-center rounded-md pe-1 hover:bg-sidebar-accent",
                root.id === canonicalActiveRootId && "bg-sidebar-primary/8",
                collectionDropTarget?.position === "root" &&
                  collectionDropTarget.rootFolderId === root.id &&
                  "bg-primary/10 ring-1 ring-inset ring-primary/45"
              )}
            >
              <button
                type="button"
                className={cn(
                  "flex h-6 w-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground",
                  !expandable && "pointer-events-none opacity-0"
                )}
                aria-label={`${isExpanded ? t("collapse") : t("expand")} ${root.name}`}
                onClick={() =>
                  setCollapsedPaths((current) => {
                    const next = new Set(current)
                    if (next.has(root.id)) next.delete(root.id)
                    else next.add(root.id)
                    return next
                  })
                }
              >
                {isExpanded ? (
                  <ChevronDown className="h-3.5 w-3.5" />
                ) : (
                  <ChevronRight className="h-3.5 w-3.5 rtl:rotate-180" />
                )}
              </button>
              <button
                type="button"
                title={`${root.name}\n${root.path}`}
                className="flex h-full min-w-0 flex-1 items-center gap-1.5 text-start text-xs font-medium"
                onClick={() =>
                  setCollapsedPaths((current) => {
                    const next = new Set(current)
                    if (next.has(root.id)) next.delete(root.id)
                    else next.add(root.id)
                    return next
                  })
                }
              >
                {isExpanded ? (
                  <FolderOpen className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                ) : (
                  <Folder className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                )}
                <span className="truncate">{root.alias || root.name}</span>
                <span className="ms-auto shrink-0 text-[10px] font-normal text-muted-foreground">
                  {sessionCountByRoot.get(root.id) ?? 0}
                </span>
              </button>
              {onNewSession ? (
                <Button
                  size="icon-sm"
                  variant="ghost"
                  className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                  aria-label={`${tSidebar("newChat")} · ${root.alias || root.name}`}
                  onClick={() => onNewSession(root.id)}
                >
                  <SquarePen className="h-3.5 w-3.5" />
                </Button>
              ) : null}
              <Button
                size="icon-sm"
                variant="ghost"
                className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                aria-label={`${t("newRoot")} · ${root.alias || root.name}`}
                onClick={() =>
                  openEditor({
                    mode: "create",
                    parentId: null,
                    rootFolderId: root.id,
                  })
                }
              >
                <FolderPlus className="h-3.5 w-3.5" />
              </Button>
            </div>
            {isExpanded ? (
              <>
                {renderItems(null, 1, root.id)}
                {renderUnclassified(root.id, sessions)}
              </>
            ) : null}
          </div>
        )}
      </TreeDropBindings>
    )
  }

  return (
    <DndContext
      sensors={dndSensors}
      collisionDetection={pointerWithin}
      onDragStart={handleTreeDragStart}
      onDragMove={handleTreeDragMove}
      onDragOver={handleTreeDragMove}
      onDragCancel={clearTreeDrag}
      onDragEnd={handleTreeDragEnd}
    >
      <section
        className={cn(
          "border-b border-border/40 px-1.5 pb-1.5",
          showSessions && "flex min-h-0 flex-1 flex-col"
        )}
      >
        <div className="flex h-7 items-center px-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
          <span className="min-w-0 flex-1 truncate">{t("title")}</span>
          <Button
            size="icon-sm"
            variant="ghost"
            className="h-6 w-6"
            aria-label={t("newRoot")}
            onClick={() => openEditor({ mode: "create", parentId: null })}
          >
            <Plus className="h-3.5 w-3.5" />
          </Button>
        </div>
        <div
          ref={scrollContainerRef}
          className={cn(
            "overflow-y-auto",
            showSessions ? "min-h-0 flex-1" : "max-h-40"
          )}
        >
          {showSessions && membershipsLoading && conversationIdsKey !== "" ? (
            <div className="flex h-7 items-center justify-center text-muted-foreground">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            </div>
          ) : null}
          {loading && items.length === 0 && pathRoots.length === 0 ? (
            <div className="flex h-8 items-center justify-center text-muted-foreground">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            </div>
          ) : showSessions ? (
            <>
              {pathRoots.map(renderPath)}
              {/* Legacy Collections created before canonical Path ownership was
                introduced stay reachable instead of silently disappearing. */}
              {renderItems(null, 0, null)}
              {pathRoots.length === 0 && items.length === 0 ? (
                <button
                  type="button"
                  className="w-full px-2 py-1.5 text-start text-xs text-muted-foreground hover:text-foreground"
                  onClick={() => openEditor({ mode: "create", parentId: null })}
                >
                  {t("empty")}
                </button>
              ) : null}
            </>
          ) : (
            <>
              <button
                type="button"
                className="flex h-7 w-full min-w-0 items-center gap-1.5 rounded-md px-2 text-start text-xs hover:bg-sidebar-accent"
                aria-label={t("unclassified")}
                onClick={() => onOpenScope("unclassified")}
              >
                <Inbox className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate">{t("unclassified")}</span>
              </button>
              {items.length === 0 ? (
                <button
                  type="button"
                  className="w-full px-2 py-1.5 text-start text-xs text-muted-foreground hover:text-foreground"
                  onClick={() => openEditor({ mode: "create", parentId: null })}
                >
                  {t("empty")}
                </button>
              ) : (
                renderItems(null)
              )}
            </>
          )}
        </div>

        {multiSelect.selected.size > 0 ? (
          <SessionBulkActionBar
            selected={multiSelect.selected}
            onClear={multiSelect.clear}
          />
        ) : null}

        <Dialog
          open={sessionRename != null}
          onOpenChange={(open) => !open && setSessionRename(null)}
        >
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{tConversation("renameConversation")}</DialogTitle>
            </DialogHeader>
            <Input
              value={sessionRename?.value ?? ""}
              onChange={(event) =>
                setSessionRename((current) =>
                  current ? { ...current, value: event.target.value } : current
                )
              }
              {...ime.props}
              onKeyDown={(event) => {
                if (ime.isComposing(event)) return
                if (event.key === "Enter") void handleSessionRename()
              }}
              autoFocus
            />
            <DialogFooter>
              <Button variant="outline" onClick={() => setSessionRename(null)}>
                {tConversation("cancel")}
              </Button>
              <Button onClick={() => void handleSessionRename()}>
                {tConversation("save")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <AlertDialog
          open={bulkDeleteOpen}
          onOpenChange={(open) => !open && setBulkDeleteOpen(false)}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>
                {tManage("confirmDeleteTitle", {
                  count: multiSelect.selected.size,
                })}
              </AlertDialogTitle>
              <AlertDialogDescription>
                {tManage("confirmDeleteDescription")}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>{tConversation("cancel")}</AlertDialogCancel>
              <AlertDialogAction onClick={() => void handleBulkDelete()}>
                {tCommon("confirm")}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>

        <AlertDialog
          open={sessionDelete != null}
          onOpenChange={(open) => !open && setSessionDelete(null)}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>
                {tConversation("deleteConversationTitle")}
              </AlertDialogTitle>
              <AlertDialogDescription>
                {tConversation("deleteConversationDescription", {
                  title:
                    formatConversationTitle(sessionDelete?.title) ||
                    tConversation("untitledConversation"),
                })}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>{tConversation("cancel")}</AlertDialogCancel>
              <AlertDialogAction onClick={() => void handleSessionDelete()}>
                {tConversation("delete")}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>

        {sessionDetails ? (
          <SessionDetailsDialog
            open
            onOpenChange={(open) => !open && setSessionDetails(null)}
            summary={sessionDetails}
          />
        ) : null}

        <Dialog
          open={editor != null}
          onOpenChange={(open) => !open && setEditor(null)}
        >
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle>
                {editor?.mode === "create"
                  ? t("createTitle")
                  : editor?.mode === "rename"
                    ? t("renameTitle")
                    : t("moveTitle")}
              </DialogTitle>
            </DialogHeader>
            <div className="space-y-3">
              {editor?.mode !== "move" ? (
                <Input
                  autoFocus
                  value={name}
                  maxLength={80}
                  placeholder={t("namePlaceholder")}
                  onChange={(event) => setName(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void submitEditor()
                  }}
                />
              ) : null}
              {editor?.mode === "move" ? (
                <Select
                  value={parentId == null ? "root" : String(parentId)}
                  onValueChange={(value) =>
                    setParentId(value === "root" ? null : Number(value))
                  }
                >
                  <SelectTrigger aria-label={t("parentLabel")}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="root">{t("root")}</SelectItem>
                    {options
                      .filter(({ item }) => !invalidMoveParents.has(item.id))
                      .map(({ item, depth }) => (
                        <SelectItem key={item.id} value={String(item.id)}>
                          {depth > 0 ? `${"· ".repeat(depth)}` : ""}
                          {item.name}
                        </SelectItem>
                      ))}
                  </SelectContent>
                </Select>
              ) : null}
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setEditor(null)}>
                {tCommon("cancel")}
              </Button>
              <Button
                disabled={pending || (editor?.mode !== "move" && !name.trim())}
                onClick={() => void submitEditor()}
              >
                {pending ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
                {tCommon("confirm")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <AlertDialog
          open={deleteTarget != null}
          onOpenChange={(open) => !open && setDeleteTarget(null)}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{t("deleteTitle")}</AlertDialogTitle>
              <AlertDialogDescription>
                {t("deleteDescription", { name: deleteTarget?.name ?? "" })}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>{tCommon("cancel")}</AlertDialogCancel>
              <AlertDialogAction
                disabled={pending}
                onClick={() => void confirmDelete()}
              >
                {tCommon("confirm")}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </section>
      {typeof document !== "undefined"
        ? createPortal(
            <DragOverlay dropAnimation={null}>
              {activeTreeDrag ? (
                <div className="flex max-w-72 items-center gap-2 rounded-md border border-primary/35 bg-popover px-3 py-2 text-xs text-popover-foreground shadow-lg">
                  {activeTreeDrag.kind === "collection" ? (
                    <Folder className="h-3.5 w-3.5 shrink-0" />
                  ) : (
                    <Circle className="h-3 w-3 shrink-0 fill-current" />
                  )}
                  <span className="truncate">
                    {activeTreeDrag.kind === "session" &&
                    sessionIdsInDrag(activeTreeDrag).length > 1
                      ? t("dragSessionCount", {
                          count: sessionIdsInDrag(activeTreeDrag).length,
                        })
                      : activeTreeDrag.label}
                  </span>
                </div>
              ) : null}
            </DragOverlay>,
            document.body
          )
        : null}
    </DndContext>
  )
})
