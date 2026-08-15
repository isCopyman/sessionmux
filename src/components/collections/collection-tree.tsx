"use client"

import { useEffect, useMemo, useState, type ReactNode } from "react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderInput,
  FolderOpen,
  FolderPlus,
  Inbox,
  Loader2,
  MoreHorizontal,
  Pencil,
  Plus,
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { AgentIcon } from "@/components/agent-icon"
import { ConversationStatusDot } from "@/components/conversations/conversation-status-dot"
import { listConversationCollectionRefs } from "@/lib/api"
import { formatConversationTitle } from "@/lib/conversation-title"
import type {
  CollectionInfo,
  ConversationStatus,
  DbConversationSummary,
} from "@/lib/types"
import { toErrorMessage } from "@/lib/app-error"
import { cn } from "@/lib/utils"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useCollectionStore } from "@/stores/collection-store"
import type { SidebarSortMode } from "@/lib/sidebar-view-mode-storage"
import { useTabStore } from "@/contexts/tab-context"

type OpenScope = number | "unclassified"
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
  /** Start a Session in the chosen canonical Path. */
  onNewSession?: (rootFolderId: number) => void
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

export function CollectionTree({
  onOpenScope,
  showSessions = false,
  showCompleted = false,
  sortMode = "created",
  refreshKey = 0,
  onOpenSession,
  onNewSession,
}: CollectionTreeProps) {
  const t = useTranslations("Folder.sidebar.collections")
  const tSidebar = useTranslations("Folder.sidebar")
  const tCommon = useTranslations("Folder.common")
  const tConversation = useTranslations("Folder.conversationCard")
  const items = useCollectionStore((state) => state.items)
  const hydrated = useCollectionStore((state) => state.hydrated)
  const loading = useCollectionStore((state) => state.loading)
  const hydrate = useCollectionStore((state) => state.hydrate)
  const create = useCollectionStore((state) => state.create)
  const rename = useCollectionStore((state) => state.rename)
  const move = useCollectionStore((state) => state.move)
  const remove = useCollectionStore((state) => state.remove)
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const folders = useAppWorkspaceStore((state) => state.folders)
  const allFolders = useAppWorkspaceStore((state) => state.allFolders)
  const activeFolderId = useAppWorkspaceStore((state) => state.activeFolderId)
  const activeTabId = useTabStore((state) => state.activeTabId)
  const tabs = useTabStore((state) => state.tabs)

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
  }, [conversationIdsKey, refreshKey, showSessions, t])

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

  const renderSession = (
    conversation: DbConversationSummary,
    depth: number
  ) => {
    const selected = conversation.id === activeConversationId
    return (
      <button
        key={conversation.id}
        type="button"
        data-conversation-id={conversation.id}
        data-focused-session={selected ? "true" : undefined}
        aria-current={selected ? "page" : undefined}
        title={formatConversationTitle(conversation.title)}
        className={cn(
          "flex h-7 w-full min-w-0 items-center gap-1.5 rounded-md pe-2 text-start text-xs hover:bg-sidebar-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
          selected &&
            "bg-primary/8 text-primary ring-1 ring-inset ring-primary/30"
        )}
        style={{ paddingInlineStart: `${1.75 + depth * 0.75}rem` }}
        onClick={() => onOpenSession?.(conversation)}
      >
        <span
          aria-hidden
          className="relative flex h-3.5 w-3.5 shrink-0 items-center justify-center"
        >
          <AgentIcon agentType={conversation.agent_type} className="h-3 w-3" />
          <ConversationStatusDot
            status={conversation.status as ConversationStatus}
            size="sm"
            className="absolute -bottom-0.5 -right-0.5 ring-1 ring-sidebar"
          />
        </span>
        <span className="truncate">
          {formatConversationTitle(conversation.title) ||
            tConversation("untitledConversation")}
        </span>
      </button>
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
          <div key={item.id}>
            <div
              className="group flex h-7 min-w-0 items-center rounded-md pe-1 hover:bg-sidebar-accent"
              style={{ paddingInlineStart: `${0.25 + depth * 0.75}rem` }}
            >
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
              <button
                type="button"
                className="flex h-full min-w-0 flex-1 items-center gap-1.5 text-start text-xs"
                title={item.name}
                aria-label={item.name}
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
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
            {isExpanded ? (
              <>
                {memberSessions.map((session) =>
                  renderSession(session, depth + 1)
                )}
                {renderItems(item.id, depth + 1, rootFolderId)}
              </>
            ) : null}
          </div>
        )
      })

  const renderUnclassified = (
    rootFolderId: number,
    sessions: DbConversationSummary[]
  ) => {
    const isExpanded = !collapsedUnclassified.has(rootFolderId)
    return (
      <div key={`unclassified:${rootFolderId}`}>
        <div className="group flex h-7 min-w-0 items-center rounded-md pe-1 ps-4 hover:bg-sidebar-accent">
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
          ? sessions.map((session) => renderSession(session, 1))
          : null}
      </div>
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
      <div key={root.id} data-collection-path={root.id}>
        <div
          className={cn(
            "group flex h-8 min-w-0 items-center rounded-md pe-1 hover:bg-sidebar-accent",
            root.id === canonicalActiveRootId && "bg-sidebar-primary/8"
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
    )
  }

  return (
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
            {editor?.mode !== "rename" ? (
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
  )
}
