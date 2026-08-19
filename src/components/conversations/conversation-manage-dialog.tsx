"use client"

import {
  Fragment,
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import {
  ArrowLeft,
  Archive,
  ArchiveRestore,
  Bot,
  Check,
  CheckSquare,
  ChevronDown,
  ChevronRight,
  GitBranch,
  FolderTree,
  ListChecks,
  Loader2,
  MessageSquareMore,
  MessageSquareText,
  PanelTopOpen,
  PanelRightOpen,
  PanelBottomOpen,
  PanelsTopLeft,
  Plus,
  Search,
  Square,
  Trash2,
  UserRound,
  Users,
  Zap,
} from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Command,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from "@/components/ui/command"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
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
import { AgentIcon } from "@/components/agent-icon"
import {
  buildBranchTree,
  expandedKeysForBranch,
  type BranchTreeLeaf,
  type BranchTreeNode,
} from "@/lib/branch-tree"
import {
  FolderSelect,
  type FolderSelectOption,
} from "@/components/shared/folder-select"
import { FolderAliasLabel } from "@/components/conversations/folder-alias-label"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useTabStore } from "@/stores/tab-store"
import { useWorkbenchStore } from "@/stores/workbench-store"
import { useCollectionStore } from "@/stores/collection-store"
import { useOrganizationRevisionStore } from "@/stores/organization-revision-store"
import { useTabActions } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import {
  deleteConversation,
  assignConversationsToCollection,
  getFolderConversationTurns,
  listAllConversations,
  listConversationCollectionRefs,
  listConversationWorkbenchRefs,
  searchSessionContent,
  updateConversationArchive,
  updateConversationStatus,
} from "@/lib/api"
import type {
  AgentType,
  CollectionInfo,
  ConversationCollectionRef,
  ConversationWorkbenchRef,
  ConversationStatus,
  DbConversationSummary,
  MessageTurn,
} from "@/lib/types"
import { ALL_AGENT_TYPES, STATUS_ORDER } from "@/lib/types"
import {
  matchesSessionSource,
  type SessionSourceFilter,
} from "@/lib/conversation-source"
import { getAgentLabel } from "@/lib/custom-agents"
import {
  excludeChatFolders,
  filterTopLevelFolders,
  formatFolderLabelWithAlias,
} from "@/lib/folder-display"
import { cn } from "@/lib/utils"
import { formatConversationTitle } from "@/lib/conversation-title"
import {
  appendConversationsToWorkbench,
  SESSION_CENTER_TAB_ORIGIN,
} from "@/lib/workbench-session-tabs"
import { toErrorMessage } from "@/lib/app-error"
import { ConversationStatusDot } from "@/components/conversations/conversation-status-dot"
import { useCollaborationUnreadOverview } from "@/hooks/use-collaboration-unread-overview"

export type CollectionFilter = "all" | "unclassified" | number

interface ConversationManageDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The folder the dialog was opened on — the initial value of the folder
   *  facet, which the user can then widen to the whole workspace or point at
   *  another folder. */
  folderId?: number | null
  /** Optional semantic scope selected from the sidebar Collection tree. */
  initialCollection?: number | "unclassified" | null
}

/**
 * What the branch facet is filtering by. `none` isolates the conversations that
 * carry no branch at all (started in a non-repo folder, or imported from an
 * agent whose session files don't record one).
 *
 * A union rather than `string | "all" | null`, because a branch can legitimately
 * BE named `all` — a sentinel string would filter to the wrong thing.
 */
type BranchFilter =
  | { kind: "all" }
  | { kind: "none" }
  | { kind: "branch"; name: string }

const ALL_BRANCHES: BranchFilter = { kind: "all" }

type WorkbenchFilter = "all" | "unopened" | number

type SessionSearchScope = "all" | "metadata" | "content"

type SessionStatusFilter = ConversationStatus | "all" | "archived"
type CollaborationFilter =
  | "all"
  | "unread"
  | "needs_reply"
  | "awaiting_reply"
  | "failed"

interface CollectionOption {
  item: CollectionInfo
  depth: number
  path: string
}

function flattenCollections(items: CollectionInfo[]): CollectionOption[] {
  const byParent = new Map<number | null, CollectionInfo[]>()
  for (const item of items) {
    const siblings = byParent.get(item.parent_id) ?? []
    siblings.push(item)
    byParent.set(item.parent_id, siblings)
  }
  for (const siblings of byParent.values()) {
    siblings.sort((a, b) => a.position - b.position || a.id - b.id)
  }

  const options: CollectionOption[] = []
  const visited = new Set<number>()
  const append = (item: CollectionInfo, depth: number, parents: string[]) => {
    if (visited.has(item.id)) return
    visited.add(item.id)
    const path = [...parents, item.name]
    options.push({ item, depth, path: path.join(" / ") })
    for (const child of byParent.get(item.id) ?? []) {
      append(child, depth + 1, path)
    }
  }
  for (const root of byParent.get(null) ?? []) append(root, 0, [])
  // Corrupt legacy rows with a missing parent stay manageable instead of
  // disappearing from every selector.
  for (const item of items) {
    if (!visited.has(item.id)) append(item, 0, [])
  }
  return options
}

function collectionDescendants(items: CollectionInfo[], rootId: number) {
  const result = new Set<number>([rootId])
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

/**
 * Shared metrics for the four facet controls, so the folder picker, the branch
 * popover and the two `Select`s read as one row of equal columns rather than
 * three unrelated widgets.
 *
 * `w-full max-w-none` is the load-bearing part: `FolderSelect`'s `field`
 * variant is content-sized (`w-auto max-w-[16rem]`), which made the row's
 * columns — and, before the search box moved to its own line, the search box
 * itself — resize every time the picked folder's name changed length.
 */
const FACET_TRIGGER_CLASS = "h-9 w-full min-w-0 max-w-none text-sm"

/**
 * Same, for the two native `Select` triggers: they ship a 16px chevron at full
 * muted strength where the folder and branch popovers beside them use a 14px
 * one at 60%. `>svg` is the chevron alone — the selected value's own glyph
 * renders deeper, inside the value slot.
 */
const FACET_SELECT_TRIGGER_CLASS = cn(
  FACET_TRIGGER_CLASS,
  "[&>svg]:size-3.5 [&>svg]:text-muted-foreground/60"
)

/**
 * A status dot in the same 14px slot the other facets' glyphs occupy — on its
 * own it is 8px, which would start the status label a few pixels left of every
 * other label in the row.
 */
function StatusGlyph({ status }: { status?: ConversationStatus }) {
  return (
    <span className="flex size-3.5 items-center justify-center">
      <ConversationStatusDot status={status} />
    </span>
  )
}

/** One branch the current rows actually use, and how many of them use it. */
interface BranchOption {
  name: string
  count: number
}

function parseTimestamp(value: string): number {
  const ts = Date.parse(value)
  return Number.isNaN(ts) ? 0 : ts
}

function formatRelative(iso: string): string {
  const ts = parseTimestamp(iso)
  if (!ts) return ""
  const diff = Math.max(0, Date.now() - ts)
  const m = Math.floor(diff / 60000)
  if (m < 1) return "now"
  if (m < 60) return `${m}m`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h`
  const d = Math.floor(h / 24)
  if (d < 30) return `${d}d`
  const mo = Math.floor(d / 30)
  if (mo < 12) return `${mo}mo`
  const y = Math.floor(mo / 12)
  return `${y}y`
}

/** Keep Session Center's preview intentionally light: it is for recognition,
 * not a second transcript renderer. Tool payloads and reasoning stay out; the
 * full, live UI only starts after the user explicitly opens the session. */
function readableTurnText(turn: MessageTurn, imageLabel: string): string {
  return turn.blocks
    .flatMap((block) => {
      if (block.type === "text") return [block.text.trim()]
      if (block.type === "image" || block.type === "image_generation") {
        return [imageLabel]
      }
      return []
    })
    .filter(Boolean)
    .join("\n")
}

/**
 * Left padding for a branch row at `depth`, measured from `CommandItem`'s own
 * `px-2` base so the tree's first level lines up with the pinned rows above it.
 * Deliberately not `branchRowPaddingLeft` from the shared tree renderer: that
 * base is tuned to the dropdown selectors' roomier `rounded-xl py-2` rows.
 */
function branchRowIndent(depth: number): string {
  return `${0.5 + depth * 0.75}rem`
}

/**
 * The prefix-grouped branch rows. Group headers are `CommandItem`s like the
 * leaves rather than plain buttons, so arrow keys reach them and Enter folds
 * them — cmdk only navigates its own items, and the shared
 * `BranchTreeCollapsible` renderer (built for Radix dropdowns) would drop its
 * triggers out of this list's keyboard path.
 */
function BranchTreeRows({
  nodes,
  depth,
  expanded,
  onToggleGroup,
  renderLeaf,
}: {
  nodes: readonly BranchTreeNode[]
  depth: number
  expanded: ReadonlySet<string>
  onToggleGroup: (key: string) => void
  renderLeaf: (leaf: BranchTreeLeaf, depth: number) => ReactNode
}) {
  return (
    <>
      {nodes.map((node) => {
        if (node.type === "leaf") {
          return <Fragment key={node.key}>{renderLeaf(node, depth)}</Fragment>
        }
        // Folded by default, like the other branch selectors: a repo whose
        // `task/` group runs to fifty worktrees would otherwise bury `main`
        // below a scroll. The count on the header says what is inside, and
        // typing flattens the whole tree anyway.
        const open = expanded.has(node.key)
        return (
          <Fragment key={node.key}>
            <CommandItem
              // Group keys are `g <scope> <prefix>` and a git ref can't contain
              // a space, so they never collide with a leaf's branch name.
              value={node.key}
              onSelect={() => onToggleGroup(node.key)}
              style={{ paddingLeft: branchRowIndent(depth) }}
            >
              <ChevronRight
                className={cn(
                  "h-4 w-4 text-muted-foreground transition-transform",
                  open && "rotate-90"
                )}
              />
              <span dir="ltr" className="min-w-0 flex-1 truncate text-start">
                {node.label}
              </span>
              <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
                {node.count}
              </span>
            </CommandItem>
            {open ? (
              <BranchTreeRows
                nodes={node.children}
                depth={depth + 1}
                expanded={expanded}
                onToggleGroup={onToggleGroup}
                renderLeaf={renderLeaf}
              />
            ) : null}
          </Fragment>
        )
      })}
    </>
  )
}

/**
 * The branch facet: the branches the matched conversations actually ran on,
 * each with its count, plus pinned "all branches" and (when some conversation
 * has none) "no branch" rows.
 *
 * Shaped like the shared `FolderSelect` — a command palette in a popover — for
 * the same reason: a repo accumulates far more branches than a plain `Select`
 * can show without scrolling blindly. Branches are prefix-grouped on `/` with
 * the same `buildBranchTree` the three git selectors use, so `task/49` and
 * `task/50` fold under one `task/` header here too.
 *
 * Filtering is ours rather than cmdk's (`shouldFilter={false}`): a typed query
 * has to flatten the tree — a collapsed group can't offer a match it isn't
 * rendering — which is exactly what the other branch selectors do.
 */
function BranchFilterSelect({
  value,
  onChange,
  branches,
  noBranchCount,
  className,
}: {
  value: BranchFilter
  onChange: (next: BranchFilter) => void
  branches: readonly BranchOption[]
  noBranchCount: number
  className?: string
}) {
  const t = useTranslations("Folder.sidebar.manageConversations")
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState("")
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set())

  const label =
    value.kind === "all"
      ? t("branchFilterAll")
      : value.kind === "none"
        ? t("branchNone")
        : value.name

  const select = (next: BranchFilter) => {
    setOpen(false)
    onChange(next)
  }

  const toggleGroup = useCallback((key: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }, [])

  const nodes = useMemo(
    () =>
      buildBranchTree(
        branches.map((b) => ({ full: b.name, display: b.name })),
        "facet"
      ),
    [branches]
  )
  const countByBranch = useMemo(
    () => new Map(branches.map((b) => [b.name, b.count])),
    [branches]
  )

  const q = query.trim().toLowerCase()
  // Searching flattens: full names, so a hit under `task/` still reads as the
  // branch it is once its group header is gone.
  const matches = useMemo(
    () => (q ? branches.filter((b) => b.name.toLowerCase().includes(q)) : []),
    [branches, q]
  )
  const noBranchLabel = t("branchNone")
  const showNoBranch =
    noBranchCount > 0 && (!q || noBranchLabel.toLowerCase().includes(q))
  const nothingMatched = q !== "" && matches.length === 0 && !showNoBranch

  /** One branch row. `label` is the remainder under its group; `fullName` is
   *  the ref the facet actually filters by. */
  const renderLeaf = (leaf: BranchTreeLeaf, depth: number) => (
    <CommandItem
      value={leaf.fullName}
      onSelect={() => select({ kind: "branch", name: leaf.fullName })}
      style={{ paddingLeft: branchRowIndent(depth) }}
    >
      <GitBranch className="h-4 w-4" />
      <span dir="ltr" className="min-w-0 flex-1 truncate text-start">
        {leaf.label}
      </span>
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
        {countByBranch.get(leaf.fullName)}
      </span>
      {value.kind === "branch" && value.name === leaf.fullName ? (
        <Check className="h-4 w-4 shrink-0" />
      ) : null}
    </CommandItem>
  )

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) return
        // Each open starts from the whole tree, not from the last search. Reset
        // on the way IN rather than out: picking a row closes the popover by
        // setting `open` directly, which Radix never reports back here.
        setQuery("")
        // Folds reset per open too, except along the path to whatever is picked
        // — reopening onto a checkmark hidden inside a collapsed group reads as
        // "nothing is selected".
        setExpanded(
          new Set(
            value.kind === "branch"
              ? expandedKeysForBranch(nodes, value.name)
              : []
          )
        )
      }}
    >
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="sm"
          title={label}
          // Looks only — the caller owns the metrics, so this trigger sizes
          // with the rest of the facet row.
          className={cn(
            "justify-between gap-1.5 rounded-4xl border-input bg-input/30 px-3 font-normal hover:bg-input/50 dark:hover:bg-input/50",
            className
          )}
        >
          <GitBranch
            className="size-3.5 shrink-0 text-muted-foreground"
            aria-hidden="true"
          />
          {/* Branch names read LTR even in RTL locales. */}
          <span
            dir={value.kind === "branch" ? "ltr" : undefined}
            className="min-w-0 flex-1 truncate text-start"
          >
            {label}
          </span>
          <ChevronDown
            className="size-3.5 shrink-0 text-muted-foreground/60"
            aria-hidden="true"
          />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-64 overflow-hidden p-0">
        <Command className="rounded-2xl" shouldFilter={false}>
          <CommandInput
            value={query}
            onValueChange={setQuery}
            placeholder={t("branchSearchPlaceholder")}
          />
          <CommandList>
            {/* Pinned above the (scrolling) branches, like the folder picker's
                "all folders" row: clearing the facet must never require
                scrolling back up, and it survives any search. */}
            <div className="sticky top-0 z-10 bg-popover">
              <CommandGroup>
                <CommandItem
                  value="__all_branches__"
                  onSelect={() => select(ALL_BRANCHES)}
                >
                  <GitBranch className="h-4 w-4" />
                  <span className="min-w-0 flex-1 truncate">
                    {t("branchFilterAll")}
                  </span>
                  {value.kind === "all" ? (
                    <Check className="h-4 w-4 shrink-0" />
                  ) : null}
                </CommandItem>
              </CommandGroup>
              <CommandSeparator />
            </div>
            {nothingMatched ? (
              <div className="py-6 text-center text-sm">
                {t("noMatchingBranches")}
              </div>
            ) : null}
            <CommandGroup>
              {showNoBranch ? (
                <CommandItem
                  value="__no_branch__"
                  onSelect={() => select({ kind: "none" })}
                >
                  <GitBranch className="h-4 w-4 opacity-50" />
                  <span className="min-w-0 flex-1 truncate text-muted-foreground">
                    {noBranchLabel}
                  </span>
                  <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
                    {noBranchCount}
                  </span>
                  {value.kind === "none" ? (
                    <Check className="h-4 w-4 shrink-0" />
                  ) : null}
                </CommandItem>
              ) : null}
              {q ? (
                matches.map((b) => (
                  <Fragment key={b.name}>
                    {renderLeaf(
                      {
                        type: "leaf",
                        fullName: b.name,
                        label: b.name,
                        key: b.name,
                      },
                      0
                    )}
                  </Fragment>
                ))
              ) : (
                <BranchTreeRows
                  nodes={nodes}
                  depth={0}
                  expanded={expanded}
                  onToggleGroup={toggleGroup}
                  renderLeaf={renderLeaf}
                />
              )}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

export function ConversationManageDialog({
  open,
  onOpenChange,
  folderId,
  initialCollection = null,
}: ConversationManageDialogProps) {
  const t = useTranslations("Folder.sidebar.manageConversations")
  const tCommon = useTranslations("Folder.common")
  const tStatus = useTranslations("Folder.statusLabels")
  const tWorkbench = useTranslations("Folder.workbench")
  const tCollaboration = useTranslations("Collaboration")
  const { overview: collaborationOverview, statusByConversation } =
    useCollaborationUnreadOverview()

  const refreshConversations = useAppWorkspaceStore(
    (s) => s.refreshConversations
  )
  const allFolders = useAppWorkspaceStore((s) => s.allFolders)
  const { closeConversationTab, openTab } = useTabActions()
  const { openConversations } = useWorkbenchRoute()
  const activeWorkbenchId = useTabStore((s) => s.activeWorkbenchId)
  const activeWorkbenchTabs = useTabStore((s) => s.rawTabs)
  const switchWorkbench = useTabStore((s) => s.switchWorkbench)
  const workbenches = useWorkbenchStore((s) => s.items)
  const workbenchesHydrated = useWorkbenchStore((s) => s.hydrated)
  const hydrateWorkbenches = useWorkbenchStore((s) => s.hydrate)
  const createOnly = useWorkbenchStore((s) => s.createOnly)
  const reopenAndSwitch = useWorkbenchStore((s) => s.reopenAndSwitch)
  const collections = useCollectionStore((s) => s.items)
  const collectionsHydrated = useCollectionStore((s) => s.hydrated)
  const hydrateCollections = useCollectionStore((s) => s.hydrate)
  const organizationRevision = useOrganizationRevisionStore(
    (state) => state.revision
  )

  const [search, setSearch] = useState("")
  const [searchScope, setSearchScope] = useState<SessionSearchScope>("all")
  /** The folder facet: a folder id, or `null` for the whole workspace. */
  const [scopeFolderId, setScopeFolderId] = useState<number | null>(
    folderId ?? null
  )
  const [branchFilter, setBranchFilter] = useState<BranchFilter>(ALL_BRANCHES)
  const [workbenchFilter, setWorkbenchFilter] = useState<WorkbenchFilter>("all")
  const [collectionFilter, setCollectionFilter] = useState<CollectionFilter>(
    initialCollection ?? "all"
  )
  const [agentFilter, setAgentFilter] = useState<AgentType | "all">("all")
  const [statusFilter, setStatusFilter] = useState<SessionStatusFilter>("all")
  const [sourceFilter, setSourceFilter] = useState<SessionSourceFilter>("all")
  const [collaborationFilter, setCollaborationFilter] =
    useState<CollaborationFilter>("all")
  // Open facet dropdowns form a layer ABOVE this dialog: while one is open an
  // Escape belongs to it (collapse the dropdown) and must not reach the dialog,
  // which would close the whole session center from under the user. The count
  // feeds DialogContent's onEscapeKeyDown guard below.
  const [openFacetMenus, setOpenFacetMenus] = useState(0)
  const trackFacetMenuOpen = useCallback((open: boolean) => {
    setOpenFacetMenus((count) => Math.max(0, count + (open ? 1 : -1)))
  }, [])
  const [rows, setRows] = useState<DbConversationSummary[]>([])
  const [contentSnippets, setContentSnippets] = useState<Map<number, string>>(
    new Map()
  )
  const [contentSearchUnavailable, setContentSearchUnavailable] =
    useState(false)
  const [workbenchRefs, setWorkbenchRefs] = useState<
    ConversationWorkbenchRef[]
  >([])
  const [workbenchRefsUnavailable, setWorkbenchRefsUnavailable] =
    useState(false)
  const [workbenchRefsLoading, setWorkbenchRefsLoading] = useState(false)
  const [collectionRefs, setCollectionRefs] = useState<
    ConversationCollectionRef[]
  >([])
  const [collectionRefsUnavailable, setCollectionRefsUnavailable] =
    useState(false)
  const [collectionRefsLoading, setCollectionRefsLoading] = useState(false)
  const [previewConversation, setPreviewConversation] =
    useState<DbConversationSummary | null>(null)
  const [previewTurns, setPreviewTurns] = useState<MessageTurn[]>([])
  const [previewLoading, setPreviewLoading] = useState(false)
  const [previewError, setPreviewError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // Keyed by id but holding the row, so a bulk op can still close the tabs of
  // conversations the current facets have since filtered out of view (the
  // selection deliberately survives a facet change).
  const [selected, setSelected] = useState<Map<number, DbConversationSummary>>(
    new Map()
  )
  const [pending, setPending] = useState(false)
  const [bulkOpening, setBulkOpening] = useState(false)
  const [openingWorkbenchId, setOpeningWorkbenchId] = useState<number | null>(
    null
  )
  const [refreshKey, setRefreshKey] = useState(0)
  const [confirmDelete, setConfirmDelete] = useState(false)

  // The folders the scope facet offers: the top-level repos the sidebar draws
  // headers for. Worktree children are reached through their parent (see
  // `queryFolderIds`), and chat folders back folderless chat mode — they never
  // appear in a user-facing folder list.
  const folderOptions = useMemo(() => {
    const options = new Map<number, FolderSelectOption>()
    for (const f of excludeChatFolders(filterTopLevelFolders(allFolders))) {
      options.set(f.id, {
        id: f.id,
        name: f.name,
        alias: f.alias,
        path: f.path,
      })
    }
    // Whatever the dialog was opened on is always offered, top-level or not:
    // with "Show worktrees" on, each worktree draws its own header and so its
    // own menu entry, and the scope would otherwise be a folder the picker can
    // name only as `#<id>` and can never return to once the scope moves.
    if (folderId != null && !options.has(folderId)) {
      const own = allFolders.find((f) => f.id === folderId)
      options.set(folderId, {
        id: folderId,
        name: own?.name ?? `#${folderId}`,
        alias: own?.alias ?? null,
        path: own?.path ?? null,
      })
    }
    return [...options.values()].sort((a, b) =>
      (a.alias ?? a.name).localeCompare(b.alias ?? b.name)
    )
  }, [allFolders, folderId])

  // A picked folder queries the parent id PLUS every worktree child folder id:
  // worktree conversations keep `folder_id = the worktree child folder`, and the
  // sidebar merges them into the parent group as a display-only redirect that
  // never rewrites `folder_id` — so `list_all`'s exact `folder_id IN (...)`
  // filter would otherwise drop them. `allFolders` includes open and closed
  // (non-deleted) folders and a worktree's `parent_id` is flattened to the root,
  // so one level of filtering is complete.
  //
  // The workspace-wide scope enumerates that same universe instead of passing
  // `null`, which on the backend means "every non-deleted folder" and would also
  // sweep in the hidden chat-mode folders no scope here can name.
  const queryFolderIds = useMemo(() => {
    if (scopeFolderId == null)
      return excludeChatFolders(allFolders).map((f) => f.id)
    const ids = [scopeFolderId]
    for (const f of allFolders) {
      if (f.parent_id === scopeFolderId) ids.push(f.id)
    }
    return ids
  }, [allFolders, scopeFolderId])

  // Reset state on open/close transitions
  useEffect(() => {
    if (!open) {
      setSearch("")
      setSearchScope("all")
      setScopeFolderId(folderId ?? null)
      setBranchFilter(ALL_BRANCHES)
      setWorkbenchFilter("all")
      setCollectionFilter(initialCollection ?? "all")
      setAgentFilter("all")
      setStatusFilter("all")
      setSourceFilter("all")
      setCollaborationFilter("all")
      setSelected(new Map())
      setConfirmDelete(false)
      setError(null)
      setContentSnippets(new Map())
      setContentSearchUnavailable(false)
      setWorkbenchRefs([])
      setWorkbenchRefsUnavailable(false)
      setWorkbenchRefsLoading(false)
      setCollectionRefs([])
      setCollectionRefsUnavailable(false)
      setCollectionRefsLoading(false)
      setPreviewConversation(null)
      setPreviewTurns([])
      setPreviewLoading(false)
      setPreviewError(null)
      setOpeningWorkbenchId(null)
      setBulkOpening(false)
    }
  }, [open, folderId, initialCollection])

  useEffect(() => {
    if (!open || workbenchesHydrated) return
    void hydrateWorkbenches().catch(() => {
      // Session rows and previews remain useful when an older server cannot
      // list named workbenches; ownership already has its own graceful fallback.
    })
  }, [hydrateWorkbenches, open, workbenchesHydrated])

  useEffect(() => {
    if (!open || collectionsHydrated) return
    void hydrateCollections().catch(() => {
      // An older server may not know Collections yet. Session Center remains
      // fully usable; its Collection facet will simply be disabled.
    })
  }, [collectionsHydrated, hydrateCollections, open])

  // Debounced data fetch. Each run owns a `cancelled` flag its cleanup trips, so
  // a reply that lands after the facets moved on is dropped rather than written
  // over the newer one. Without it a slow folder-A request finishing behind
  // folder B's would leave the dialog listing A's conversations under B's
  // scope — with no folder column to give it away, since that only appears in
  // the workspace-wide scope — and Delete there would hit rows from a folder the
  // user had already navigated away from.
  useEffect(() => {
    if (!open) return
    let cancelled = false
    const timer = setTimeout(async () => {
      // Nothing to query: the workspace-wide scope over an empty folder list.
      // The backend reads an empty id list as "every non-deleted folder", so
      // asking would answer with exactly the hidden chat-mode folders this
      // dialog excludes.
      if (queryFolderIds.length === 0) {
        setRows([])
        setContentSnippets(new Map())
        setContentSearchUnavailable(false)
        setError(null)
        setLoading(false)
        return
      }
      setLoading(true)
      try {
        const normalizedSearch = search.trim()
        const metadataSearchEnabled =
          normalizedSearch === "" || searchScope !== "content"
        const contentSearchEnabled =
          normalizedSearch.length >= 2 && searchScope !== "metadata"
        const [metadataResult, contentResult] = await Promise.allSettled([
          metadataSearchEnabled
            ? listAllConversations({
                folder_ids: queryFolderIds,
                search: normalizedSearch || null,
                agent_type: agentFilter === "all" ? null : agentFilter,
                status:
                  statusFilter === "all" || statusFilter === "archived"
                    ? null
                    : statusFilter,
                archived: statusFilter === "archived",
              })
            : Promise.resolve([] as DbConversationSummary[]),
          contentSearchEnabled
            ? searchSessionContent({
                query: normalizedSearch,
                folder_ids: queryFolderIds,
                agent_type: agentFilter === "all" ? null : agentFilter,
                archived: statusFilter === "archived",
                limit: 50,
              })
            : Promise.resolve(null),
        ])
        if (cancelled) return
        if (metadataResult.status === "rejected") {
          throw metadataResult.reason
        }
        const snippets = new Map<number, string>()
        const merged = new Map(
          metadataResult.value.map((conversation) => [
            conversation.id,
            conversation,
          ])
        )
        if (
          contentResult.status === "fulfilled" &&
          contentResult.value?.available
        ) {
          for (const hit of contentResult.value.results) {
            if (
              statusFilter !== "all" &&
              statusFilter !== "archived" &&
              hit.conversation.status !== statusFilter
            ) {
              continue
            }
            merged.set(hit.conversation.id, hit.conversation)
            snippets.set(hit.conversation.id, hit.snippet)
          }
        }
        setContentSnippets(snippets)
        setContentSearchUnavailable(
          contentSearchEnabled &&
            (contentResult.status === "rejected" ||
              contentResult.value?.available === false)
        )
        const sorted = [...merged.values()].sort(
          (a, b) => parseTimestamp(b.created_at) - parseTimestamp(a.created_at)
        )
        setRows(sorted)
        setError(null)
      } catch (e) {
        if (cancelled) return
        setContentSnippets(new Map())
        setContentSearchUnavailable(false)
        setError(toErrorMessage(e))
      } finally {
        // A superseded run must leave the flag alone: its successor is in
        // flight (or one debounce away), and the list is still loading.
        if (!cancelled) setLoading(false)
      }
    }, 300)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [
    open,
    queryFolderIds,
    search,
    searchScope,
    agentFilter,
    statusFilter,
    refreshKey,
  ])

  // Workbench ownership is fetched in one batch and never blocks the session
  // rows themselves. Older servers can lack the endpoint; that degrades to an
  // explicit "unavailable" note rather than making Session Center unusable.
  useEffect(() => {
    if (!open || rows.length === 0) {
      setWorkbenchRefs([])
      setWorkbenchRefsUnavailable(false)
      setWorkbenchRefsLoading(false)
      return
    }
    let cancelled = false
    setWorkbenchRefsLoading(true)
    listConversationWorkbenchRefs(rows.map((row) => row.id))
      .then((refs) => {
        if (cancelled) return
        setWorkbenchRefs(refs)
        setWorkbenchRefsUnavailable(false)
      })
      .catch(() => {
        if (cancelled) return
        setWorkbenchRefs([])
        setWorkbenchRefsUnavailable(true)
      })
      .finally(() => {
        if (!cancelled) setWorkbenchRefsLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [open, organizationRevision, rows])

  useEffect(() => {
    if (!open || rows.length === 0) {
      setCollectionRefs([])
      setCollectionRefsUnavailable(false)
      setCollectionRefsLoading(false)
      return
    }
    let cancelled = false
    setCollectionRefsLoading(true)
    listConversationCollectionRefs(rows.map((row) => row.id))
      .then((refs) => {
        if (cancelled) return
        setCollectionRefs(refs)
        setCollectionRefsUnavailable(false)
      })
      .catch(() => {
        if (cancelled) return
        setCollectionRefs([])
        setCollectionRefsUnavailable(true)
      })
      .finally(() => {
        if (!cancelled) setCollectionRefsLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [open, organizationRevision, rows])

  const collectionOptions = useMemo(
    () => flattenCollections(collections),
    [collections]
  )
  const collectionById = useMemo(
    () => new Map(collections.map((item) => [item.id, item])),
    [collections]
  )
  const collectionRefByConversation = useMemo(
    () => new Map(collectionRefs.map((ref) => [ref.conversation_id, ref])),
    [collectionRefs]
  )
  const collectionScopeIds = useMemo(
    () =>
      typeof collectionFilter === "number"
        ? collectionDescendants(collections, collectionFilter)
        : null,
    [collectionFilter, collections]
  )

  const effectiveWorkbenchRefs = useMemo(() => {
    const refs = [...workbenchRefs]
    const seen = new Set(
      refs.map((ref) => `${ref.conversation_id}:${ref.workbench_id}`)
    )
    const activeWorkbench = workbenches.find(
      (workbench) => workbench.id === activeWorkbenchId
    )
    if (activeWorkbench) {
      for (const tab of activeWorkbenchTabs) {
        if (tab.conversationId == null) continue
        const key = `${tab.conversationId}:${activeWorkbench.id}`
        if (seen.has(key)) continue
        seen.add(key)
        refs.push({
          conversation_id: tab.conversationId,
          workbench_id: activeWorkbench.id,
          workbench_name: activeWorkbench.name,
          workbench_position: activeWorkbench.position,
        })
      }
    }
    return refs.sort(
      (a, b) =>
        a.conversation_id - b.conversation_id ||
        a.workbench_position - b.workbench_position ||
        a.workbench_id - b.workbench_id
    )
  }, [activeWorkbenchId, activeWorkbenchTabs, workbenchRefs, workbenches])

  const workbenchRefsByConversation = useMemo(() => {
    const byConversation = new Map<number, ConversationWorkbenchRef[]>()
    for (const ref of effectiveWorkbenchRefs) {
      const refs = byConversation.get(ref.conversation_id) ?? []
      refs.push(ref)
      byConversation.set(ref.conversation_id, refs)
    }
    return byConversation
  }, [effectiveWorkbenchRefs])

  // Previewing parses a small tail of the saved transcript through the
  // read-only history endpoint. It neither creates an ACP connection nor
  // resumes the Harness. The cancellation guard prevents a slow first click
  // from replacing the preview chosen by a later click.
  useEffect(() => {
    if (!open || !previewConversation) {
      setPreviewTurns([])
      setPreviewLoading(false)
      setPreviewError(null)
      return
    }
    let cancelled = false
    setPreviewTurns([])
    setPreviewLoading(true)
    setPreviewError(null)
    getFolderConversationTurns(previewConversation.id, 2_147_483_647, 12)
      .then((page) => {
        if (!cancelled) setPreviewTurns(page.turns)
      })
      .catch((error) => {
        if (!cancelled) setPreviewError(toErrorMessage(error))
      })
      .finally(() => {
        if (!cancelled) setPreviewLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [open, previewConversation])

  // Branch options are derived from the rows the other facets already matched,
  // so every branch listed is guaranteed to yield at least one conversation —
  // and the list can never disagree with what the filter then selects.
  const { branchOptions, noBranchCount } = useMemo(() => {
    const counts = new Map<string, number>()
    let none = 0
    for (const r of rows) {
      if (r.git_branch)
        counts.set(r.git_branch, (counts.get(r.git_branch) ?? 0) + 1)
      else none++
    }
    return {
      branchOptions: [...counts.entries()]
        .map(([name, count]) => ({ name, count }))
        .sort((a, b) => a.name.localeCompare(b.name)),
      noBranchCount: none,
    }
  }, [rows])

  const { workbenchCounts, unopenedCount } = useMemo(() => {
    const counts = new Map<number, number>()
    let unopened = 0
    for (const row of rows) {
      const refs = workbenchRefsByConversation.get(row.id) ?? []
      if (refs.length === 0) unopened++
      for (const ref of refs) {
        counts.set(ref.workbench_id, (counts.get(ref.workbench_id) ?? 0) + 1)
      }
    }
    return { workbenchCounts: counts, unopenedCount: unopened }
  }, [rows, workbenchRefsByConversation])

  // Branch is the first facet applied client-side: it shares its source of
  // truth with the option list above, and the rows are already in memory. The
  // Collection, workbench, source and worklist facets join it below for the
  // same reason — `list_all_conversations` has a parameter for none of them.
  const visibleRows = useMemo(() => {
    let matched: DbConversationSummary[]
    switch (branchFilter.kind) {
      case "all":
        matched = rows
        break
      case "none":
        matched = rows.filter((r) => !r.git_branch)
        break
      case "branch":
        matched = rows.filter((r) => r.git_branch === branchFilter.name)
        break
    }
    if (workbenchFilter === "unopened") {
      matched = matched.filter(
        (row) => !workbenchRefsByConversation.has(row.id)
      )
    } else if (typeof workbenchFilter === "number") {
      matched = matched.filter((row) =>
        workbenchRefsByConversation
          .get(row.id)
          ?.some((ref) => ref.workbench_id === workbenchFilter)
      )
    }

    if (collectionFilter === "unclassified") {
      matched = matched.filter(
        (row) => !collectionRefByConversation.has(row.id)
      )
    } else if (collectionScopeIds) {
      matched = matched.filter((row) => {
        const ref = collectionRefByConversation.get(row.id)
        return ref ? collectionScopeIds.has(ref.collection_id) : false
      })
    }

    if (sourceFilter !== "all") {
      matched = matched.filter((row) => matchesSessionSource(row, sourceFilter))
    }

    if (collaborationFilter !== "all") {
      matched = matched.filter((row) => {
        const status = statusByConversation.get(row.id)
        switch (collaborationFilter) {
          case "unread":
            return (status?.unreadCount ?? 0) > 0
          case "needs_reply":
            return (status?.needsReplyCount ?? 0) > 0
          case "awaiting_reply":
            return (status?.awaitingReplyCount ?? 0) > 0
          case "failed":
            return (status?.failedCount ?? 0) > 0
        }
      })
    }
    return matched
  }, [
    branchFilter,
    collaborationFilter,
    collectionFilter,
    collectionRefByConversation,
    collectionScopeIds,
    rows,
    sourceFilter,
    statusByConversation,
    workbenchFilter,
    workbenchRefsByConversation,
  ])

  useEffect(() => {
    if (
      previewConversation &&
      !visibleRows.some((row) => row.id === previewConversation.id)
    ) {
      setPreviewConversation(null)
    }
  }, [previewConversation, visibleRows])

  // Only worth a column when rows can come from more than one folder; inside a
  // single folder it would repeat the scope pill on every line.
  const showFolderColumn = scopeFolderId == null
  const folderById = useMemo(
    () => new Map(allFolders.map((f) => [f.id, f])),
    [allFolders]
  )
  const previewFolder = previewConversation
    ? folderById.get(previewConversation.folder_id)
    : undefined
  const previewWorkbenchRefs = previewConversation
    ? (workbenchRefsByConversation.get(previewConversation.id) ?? [])
    : []
  const previewCollection = previewConversation
    ? collectionById.get(
        collectionRefByConversation.get(previewConversation.id)
          ?.collection_id ?? -1
      )
    : undefined
  const previewMessages = useMemo(
    () =>
      previewTurns
        .map((turn) => ({
          turn,
          text: readableTurnText(turn, t("imageAttachment")),
        }))
        .filter((item) => item.text)
        .slice(-8),
    [previewTurns, t]
  )

  const handleScopeChange = useCallback((next: number | null) => {
    setScopeFolderId(next)
    // Branches are per-folder: carrying `feature/x` over to a folder that never
    // had it would show an empty list under a filter the user never chose there.
    setBranchFilter(ALL_BRANCHES)
  }, [])

  const toggleOne = useCallback((conv: DbConversationSummary) => {
    setSelected((prev) => {
      const next = new Map(prev)
      if (next.has(conv.id)) next.delete(conv.id)
      else next.set(conv.id, conv)
      return next
    })
  }, [])

  const allVisibleSelected = useMemo(
    () =>
      visibleRows.length > 0 && visibleRows.every((r) => selected.has(r.id)),
    [visibleRows, selected]
  )

  const toggleSelectAll = useCallback(() => {
    if (allVisibleSelected) {
      setSelected((prev) => {
        const next = new Map(prev)
        for (const r of visibleRows) next.delete(r.id)
        return next
      })
    } else {
      setSelected((prev) => {
        const next = new Map(prev)
        for (const r of visibleRows) next.set(r.id, r)
        return next
      })
    }
  }, [allVisibleSelected, visibleRows])

  const afterBulkOp = useCallback(() => {
    setSelected(new Map())
    setRefreshKey((k) => k + 1)
    refreshConversations()
  }, [refreshConversations])

  const selectedConversations = useMemo(
    () => [...selected.values()],
    [selected]
  )
  const selectedCount = selected.size

  const openConversation = useCallback(
    async (
      conversation: DbConversationSummary,
      targetWorkbenchId = activeWorkbenchId,
      split?: "right" | "down"
    ) => {
      setOpeningWorkbenchId(targetWorkbenchId)
      try {
        if (targetWorkbenchId !== activeWorkbenchId) {
          await switchWorkbench(targetWorkbenchId)
        }
        openConversations()
        const title = formatConversationTitle(conversation.title)
        if (split) {
          openTab(
            conversation.folder_id,
            conversation.id,
            conversation.agent_type,
            true,
            title,
            { split }
          )
        } else {
          openTab(
            conversation.folder_id,
            conversation.id,
            conversation.agent_type,
            true,
            title
          )
        }
        onOpenChange(false)
      } catch (error) {
        toast.error(t("toastOpFailed", { message: toErrorMessage(error) }))
      } finally {
        setOpeningWorkbenchId(null)
      }
    },
    [
      activeWorkbenchId,
      onOpenChange,
      openConversations,
      openTab,
      switchWorkbench,
      t,
    ]
  )

  const activeWorkbenchName =
    workbenches.find((workbench) => workbench.id === activeWorkbenchId)?.name ??
    t("currentWorkbench")

  const notifyAddedToWorkbench = useCallback(
    (opts: {
      workbenchId: number
      workbenchName: string
      added: number
      skipped: number
      stayInDialog: boolean
    }) => {
      const message =
        opts.added === 0
          ? t("toastAlreadyInWorkbench", { workbench: opts.workbenchName })
          : opts.skipped > 0
            ? t("toastAddedWithSkipped", {
                added: opts.added,
                skipped: opts.skipped,
                workbench: opts.workbenchName,
              })
            : t("toastAddedToWorkbench", {
                count: opts.added,
                workbench: opts.workbenchName,
              })
      toast.success(
        message,
        opts.stayInDialog && opts.added > 0
          ? {
              action: {
                label: t("openWorkbench"),
                onClick: () => {
                  onOpenChange(false)
                  void reopenAndSwitch(opts.workbenchId).catch((error) => {
                    toast.error(
                      t("toastOpFailed", {
                        message: toErrorMessage(error),
                      })
                    )
                  })
                },
              },
            }
          : undefined
      )
    },
    [onOpenChange, reopenAndSwitch, t]
  )

  const handleAddSelectedToWorkbench = useCallback(
    async (target: "current" | "new" | number, split?: "right" | "down") => {
      if (selectedConversations.length === 0) return
      if (split) {
        const only = selectedConversations[0]
        if (!only || selectedConversations.length !== 1) return
        await openConversation(only, activeWorkbenchId, split)
        return
      }

      setBulkOpening(true)
      try {
        let workbenchId = activeWorkbenchId
        let workbenchName = activeWorkbenchName
        if (target === "new") {
          const created = await createOnly(
            tWorkbench("defaultName", {
              number: workbenches.length + 1,
            })
          )
          workbenchId = created.id
          workbenchName = created.name
        } else if (target !== "current") {
          workbenchId = target
          workbenchName =
            workbenches.find((workbench) => workbench.id === target)?.name ??
            String(target)
        }

        const stayInDialog = workbenchId !== activeWorkbenchId
        if (!stayInDialog) {
          const present = new Set(
            activeWorkbenchTabs
              .map((tab) => tab.conversationId)
              .filter((id): id is number => id != null)
          )
          const toAdd = selectedConversations.filter(
            (conversation) => !present.has(conversation.id)
          )
          const skipped = selectedConversations.length - toAdd.length
          if (toAdd.length === 0) {
            notifyAddedToWorkbench({
              workbenchId,
              workbenchName,
              added: 0,
              skipped,
              stayInDialog: false,
            })
            return
          }
          openConversations()
          for (const conversation of toAdd) {
            openTab(
              conversation.folder_id,
              conversation.id,
              conversation.agent_type,
              true,
              formatConversationTitle(conversation.title)
            )
          }
          notifyAddedToWorkbench({
            workbenchId,
            workbenchName,
            added: toAdd.length,
            skipped,
            stayInDialog: false,
          })
          onOpenChange(false)
          return
        }

        const result = await appendConversationsToWorkbench(
          workbenchId,
          selectedConversations,
          SESSION_CENTER_TAB_ORIGIN
        )
        if (result.added > 0) {
          try {
            const refs = await listConversationWorkbenchRefs(
              rows.map((row) => row.id)
            )
            setWorkbenchRefs(refs)
          } catch {
            // Ownership chips can stay stale until the next fetch.
          }
        }
        notifyAddedToWorkbench({
          workbenchId,
          workbenchName,
          added: result.added,
          skipped: result.skipped,
          stayInDialog: true,
        })
      } catch (error) {
        toast.error(t("toastOpFailed", { message: toErrorMessage(error) }))
      } finally {
        setBulkOpening(false)
      }
    },
    [
      activeWorkbenchId,
      activeWorkbenchName,
      activeWorkbenchTabs,
      createOnly,
      notifyAddedToWorkbench,
      onOpenChange,
      openConversation,
      openConversations,
      openTab,
      rows,
      selectedConversations,
      t,
      tWorkbench,
      workbenches,
    ]
  )

  const handleBulkDelete = useCallback(async () => {
    if (selectedConversations.length === 0) return
    setPending(true)
    try {
      await Promise.all(
        selectedConversations.map((conv) => deleteConversation(conv.id))
      )
      for (const conv of selectedConversations) {
        closeConversationTab(conv.folder_id, conv.id, conv.agent_type)
      }
      toast.success(t("toastDeleted", { count: selectedConversations.length }))
      afterBulkOp()
    } catch (e) {
      toast.error(
        t("toastOpFailed", {
          message: toErrorMessage(e),
        })
      )
    } finally {
      setPending(false)
      setConfirmDelete(false)
    }
  }, [selectedConversations, closeConversationTab, t, afterBulkOp])

  const handleBulkStatus = useCallback(
    async (status: ConversationStatus) => {
      if (selectedConversations.length === 0) return
      setPending(true)
      try {
        await Promise.all(
          selectedConversations.map((conv) =>
            updateConversationStatus(conv.id, status)
          )
        )
        toast.success(
          t("toastStatusUpdated", { count: selectedConversations.length })
        )
        afterBulkOp()
      } catch (e) {
        toast.error(
          t("toastOpFailed", {
            message: toErrorMessage(e),
          })
        )
      } finally {
        setPending(false)
      }
    },
    [selectedConversations, t, afterBulkOp]
  )

  const handleBulkArchive = useCallback(
    async (archived: boolean) => {
      if (selectedConversations.length === 0) return
      setPending(true)
      try {
        await Promise.all(
          selectedConversations.map((conv) =>
            updateConversationArchive(conv.id, archived)
          )
        )
        toast.success(
          archived
            ? t("toastArchived", { count: selectedConversations.length })
            : t("toastRestored", { count: selectedConversations.length })
        )
        afterBulkOp()
      } catch (e) {
        toast.error(t("toastOpFailed", { message: toErrorMessage(e) }))
      } finally {
        setPending(false)
      }
    },
    [afterBulkOp, selectedConversations, t]
  )

  const handleBulkCollection = useCallback(
    async (collectionId: number | null) => {
      if (selectedConversations.length === 0) return
      setPending(true)
      try {
        const movedIds = new Set(selectedConversations.map((item) => item.id))
        const refs = await assignConversationsToCollection(
          [...movedIds],
          collectionId
        )
        setCollectionRefs((current) => [
          ...current.filter((ref) => !movedIds.has(ref.conversation_id)),
          ...refs,
        ])
        toast.success(
          t("toastCollectionMoved", { count: selectedConversations.length })
        )
        setSelected(new Map())
      } catch (e) {
        toast.error(t("toastOpFailed", { message: toErrorMessage(e) }))
      } finally {
        setPending(false)
      }
    },
    [selectedConversations, t]
  )

  const anyFacetNarrows =
    search.trim() !== "" ||
    branchFilter.kind !== "all" ||
    collectionFilter !== "all" ||
    workbenchFilter !== "all" ||
    agentFilter !== "all" ||
    statusFilter !== "all" ||
    sourceFilter !== "all" ||
    collaborationFilter !== "all"

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent
          className="flex h-[min(46rem,calc(100dvh-2rem))] w-[calc(100vw-2rem)] max-w-6xl flex-col overflow-hidden"
          onEscapeKeyDown={(event) => {
            // An open facet dropdown eats this Escape; the next one closes us.
            if (openFacetMenus > 0) event.preventDefault()
          }}
        >
          <DialogHeader>
            {/* Which folder is in scope is the folder pill's job now, not the
                title's — the pill names it alias-aware and can also read "all
                folders", which no fixed title could. */}
            <DialogTitle>{t("title")}</DialogTitle>
          </DialogHeader>

          {/* Facets: free text owns the top line — it is the one that wants
              every pixel — and the four narrowing controls line up as equal
              columns beneath it, ordered coarse to fine. */}
          <div className="flex flex-col gap-2">
            <div className="flex min-w-0 gap-2">
              <div className="relative min-w-0 flex-1">
                <Search
                  className="pointer-events-none absolute start-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground"
                  aria-hidden="true"
                />
                <Input
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder={t("searchPlaceholder")}
                  className="h-9 ps-9"
                />
              </div>
              <Select
                value={searchScope}
                onOpenChange={trackFacetMenuOpen}
                onValueChange={(value) =>
                  setSearchScope(value as SessionSearchScope)
                }
              >
                <SelectTrigger
                  className="h-9 w-[8.75rem] shrink-0 sm:w-[10.5rem]"
                  aria-label={t("searchScopeLabel")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="end">
                  <SelectItem value="all">{t("searchScopeAll")}</SelectItem>
                  <SelectItem value="metadata">
                    {t("searchScopeMetadata")}
                  </SelectItem>
                  <SelectItem value="content">
                    {t("searchScopeContent")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            {contentSearchUnavailable && (
              <p className="px-1 text-xs text-muted-foreground">
                {t("contentSearchUnavailable")}
              </p>
            )}
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-8">
              <FolderSelect
                folders={folderOptions}
                value={scopeFolderId}
                onChange={handleScopeChange}
                allLabel={t("folderFilterAll")}
                onSelectAll={() => handleScopeChange(null)}
                variant="field"
                className={FACET_TRIGGER_CLASS}
              />
              <Select
                value={
                  typeof collectionFilter === "number"
                    ? `collection:${collectionFilter}`
                    : collectionFilter
                }
                disabled={collectionRefsLoading || collectionRefsUnavailable}
                onOpenChange={trackFacetMenuOpen}
                onValueChange={(value) => {
                  if (value === "all" || value === "unclassified") {
                    setCollectionFilter(value)
                    return
                  }
                  setCollectionFilter(Number(value.slice("collection:".length)))
                }}
              >
                <SelectTrigger
                  className={FACET_SELECT_TRIGGER_CLASS}
                  aria-label={t("collectionFilterLabel")}
                  title={
                    collectionRefsUnavailable
                      ? t("collectionUnavailable")
                      : undefined
                  }
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    <span className="flex items-center gap-2">
                      <FolderTree className="h-3.5 w-3.5 text-muted-foreground" />
                      {t("collectionFilterAll")}
                    </span>
                  </SelectItem>
                  <SelectItem value="unclassified">
                    <span className="flex items-center gap-2">
                      <FolderTree className="h-3.5 w-3.5 text-muted-foreground/50" />
                      {t("collectionUnclassified")}
                    </span>
                  </SelectItem>
                  {collectionOptions.map(({ item, depth, path }) => (
                    <SelectItem
                      key={item.id}
                      value={`collection:${item.id}`}
                      title={path}
                    >
                      <span className="flex min-w-0 items-center gap-2">
                        <FolderTree className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                        <span className="truncate">
                          {depth > 0 ? `${"· ".repeat(depth)}` : ""}
                          {item.name}
                        </span>
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Select
                value={
                  typeof workbenchFilter === "number"
                    ? `workbench:${workbenchFilter}`
                    : workbenchFilter
                }
                disabled={workbenchRefsLoading || workbenchRefsUnavailable}
                onOpenChange={trackFacetMenuOpen}
                onValueChange={(value) => {
                  if (value === "all" || value === "unopened") {
                    setWorkbenchFilter(value)
                    return
                  }
                  setWorkbenchFilter(Number(value.slice("workbench:".length)))
                }}
              >
                <SelectTrigger
                  className={FACET_SELECT_TRIGGER_CLASS}
                  aria-label={t("workbenchFilterLabel")}
                  title={
                    workbenchRefsUnavailable
                      ? t("workbenchOwnershipUnavailable")
                      : undefined
                  }
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    <span className="flex items-center gap-2">
                      <PanelsTopLeft className="h-3.5 w-3.5 text-muted-foreground" />
                      {t("workbenchFilterAll")}
                    </span>
                  </SelectItem>
                  <SelectItem value="unopened">
                    <span className="flex min-w-0 items-center gap-2">
                      <PanelsTopLeft className="h-3.5 w-3.5 text-muted-foreground/50" />
                      <span className="min-w-0 flex-1 truncate">
                        {t("workbenchFilterUnopened")}
                      </span>
                      <span className="text-xs text-muted-foreground tabular-nums">
                        {unopenedCount}
                      </span>
                    </span>
                  </SelectItem>
                  {workbenches.map((workbench) => (
                    <SelectItem
                      key={workbench.id}
                      value={`workbench:${workbench.id}`}
                    >
                      <span className="flex min-w-0 items-center gap-2">
                        <PanelsTopLeft className="h-3.5 w-3.5 text-muted-foreground" />
                        <span className="min-w-0 flex-1 truncate">
                          {workbench.name}
                          {workbench.id === activeWorkbenchId
                            ? ` · ${t("currentWorkbench")}`
                            : ""}
                        </span>
                        <span className="text-xs text-muted-foreground tabular-nums">
                          {workbenchCounts.get(workbench.id) ?? 0}
                        </span>
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <BranchFilterSelect
                value={branchFilter}
                onChange={setBranchFilter}
                branches={branchOptions}
                noBranchCount={noBranchCount}
                className={FACET_TRIGGER_CLASS}
              />
              <Select
                value={agentFilter}
                onOpenChange={trackFacetMenuOpen}
                onValueChange={(v) => setAgentFilter(v as AgentType | "all")}
              >
                <SelectTrigger className={FACET_SELECT_TRIGGER_CLASS}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {/* Every row — this one included — leads with a glyph, so the
                      list (and the trigger it mirrors into) has one left edge
                      instead of two. */}
                  <SelectItem value="all">
                    <span className="flex items-center gap-2">
                      <Bot className="h-3.5 w-3.5 text-muted-foreground" />
                      {t("agentFilterAll")}
                    </span>
                  </SelectItem>
                  {ALL_AGENT_TYPES.map((at) => (
                    <SelectItem key={at} value={at}>
                      <span className="flex items-center gap-2">
                        <AgentIcon agentType={at} className="h-3.5 w-3.5" />
                        {getAgentLabel(at)}
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {/* Who started the Session, next to which agent ran it. Applied
                  client-side over the fetched rows: `list_all_conversations`
                  takes no `created_by`, and a Session whose row predates the
                  column reads as user-created (see `conversationSource`). */}
              <Select
                value={sourceFilter}
                onOpenChange={trackFacetMenuOpen}
                onValueChange={(v) => setSourceFilter(v as SessionSourceFilter)}
              >
                <SelectTrigger
                  className={FACET_SELECT_TRIGGER_CLASS}
                  aria-label={t("sourceFilterLabel")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    <span className="flex items-center gap-2">
                      <Users className="h-3.5 w-3.5 text-muted-foreground" />
                      {t("sourceFilterAll")}
                    </span>
                  </SelectItem>
                  <SelectItem value="user">
                    <span className="flex items-center gap-2">
                      <UserRound className="h-3.5 w-3.5 text-muted-foreground" />
                      {t("sourceFilterUser")}
                    </span>
                  </SelectItem>
                  <SelectItem value="agent">
                    <span className="flex items-center gap-2">
                      <Bot className="h-3.5 w-3.5 text-muted-foreground" />
                      {t("sourceFilterAgent")}
                    </span>
                  </SelectItem>
                  <SelectItem value="automation">
                    <span className="flex items-center gap-2">
                      <Zap className="h-3.5 w-3.5 text-muted-foreground" />
                      {t("sourceFilterAutomation")}
                    </span>
                  </SelectItem>
                </SelectContent>
              </Select>
              <Select
                value={statusFilter}
                onOpenChange={trackFacetMenuOpen}
                onValueChange={(v) => setStatusFilter(v as SessionStatusFilter)}
              >
                <SelectTrigger
                  className={FACET_SELECT_TRIGGER_CLASS}
                  aria-label={t("statusFilterLabel")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    <span className="flex items-center gap-2">
                      {/* Statusless: the dot's grey fallback, which no real
                          status uses (see STATUS_COLORS). */}
                      <StatusGlyph />
                      {t("statusFilterAll")}
                    </span>
                  </SelectItem>
                  {STATUS_ORDER.map((s) => (
                    <SelectItem key={s} value={s}>
                      <span className="flex items-center gap-2">
                        <StatusGlyph status={s} />
                        {tStatus(s)}
                      </span>
                    </SelectItem>
                  ))}
                  <SelectItem value="archived">
                    <span className="flex items-center gap-2">
                      <Archive className="h-3.5 w-3.5 text-muted-foreground" />
                      {t("archiveFilter")}
                    </span>
                  </SelectItem>
                </SelectContent>
              </Select>
              <Select
                value={collaborationFilter}
                onOpenChange={trackFacetMenuOpen}
                onValueChange={(value) =>
                  setCollaborationFilter(value as CollaborationFilter)
                }
              >
                <SelectTrigger
                  className={FACET_SELECT_TRIGGER_CLASS}
                  aria-label={tCollaboration("worklistFilterLabel")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="all">
                    <span className="flex items-center gap-2">
                      <MessageSquareMore className="h-3.5 w-3.5 text-muted-foreground" />
                      {tCollaboration("worklistFilterAll")}
                    </span>
                  </SelectItem>
                  <SelectItem value="unread">
                    {tCollaboration("worklistUnread", {
                      count: collaborationOverview.totalUnreadCount,
                    })}
                  </SelectItem>
                  <SelectItem value="needs_reply">
                    {tCollaboration("worklistNeedsReply", {
                      count: collaborationOverview.totalNeedsReplyCount,
                    })}
                  </SelectItem>
                  <SelectItem value="awaiting_reply">
                    {tCollaboration("worklistAwaitingReply", {
                      count: collaborationOverview.totalAwaitingReplyCount,
                    })}
                  </SelectItem>
                  <SelectItem value="failed">
                    {tCollaboration("worklistFailed", {
                      count: collaborationOverview.totalFailedCount,
                    })}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="grid min-h-0 flex-1 gap-3 md:grid-cols-[minmax(0,1.15fr)_minmax(18rem,0.85fr)]">
            {/* List container: select-all header + scrollable list */}
            <div
              className={cn(
                "min-h-0 min-w-0 flex-col overflow-hidden rounded-md border border-border/50",
                previewConversation ? "hidden md:flex" : "flex"
              )}
            >
              <div className="flex items-center justify-between px-3 py-2 border-b border-border/50 bg-muted/20">
                <button
                  type="button"
                  onClick={toggleSelectAll}
                  disabled={visibleRows.length === 0}
                  className="flex items-center gap-2 text-xs text-muted-foreground hover:text-foreground disabled:opacity-50"
                >
                  <span className="flex h-5 w-5 items-center justify-center">
                    {allVisibleSelected ? (
                      <CheckSquare className="h-4 w-4 text-primary" />
                    ) : (
                      <Square className="h-4 w-4" />
                    )}
                  </span>
                  {allVisibleSelected
                    ? t("deselectAll")
                    : t("selectAllVisible")}
                </button>
                <span className="text-xs text-muted-foreground">
                  {t("matchedCount", { count: visibleRows.length })}
                </span>
              </div>
              <ScrollArea className="min-h-0 flex-1">
                <div className="flex flex-col gap-0.5 p-1">
                  {loading ? (
                    Array.from({ length: 6 }).map((_, i) => (
                      <Skeleton key={i} className="h-9 w-full rounded-md" />
                    ))
                  ) : error ? (
                    <p className="text-destructive text-sm px-3 py-6 text-center">
                      {error}
                    </p>
                  ) : visibleRows.length === 0 ? (
                    <p className="text-muted-foreground text-sm px-3 py-6 text-center">
                      {anyFacetNarrows
                        ? t("noMatchingConversations")
                        : scopeFolderId == null
                          ? t("noConversationsWorkspace")
                          : t("noConversations")}
                    </p>
                  ) : (
                    visibleRows.map((conv) => {
                      const checked = selected.has(conv.id)
                      const focused = previewConversation?.id === conv.id
                      const folder = folderById.get(conv.folder_id)
                      const refs =
                        workbenchRefsByConversation.get(conv.id) ?? []
                      const collection = collectionById.get(
                        collectionRefByConversation.get(conv.id)
                          ?.collection_id ?? -1
                      )
                      const collaboration = statusByConversation.get(conv.id)
                      return (
                        <div
                          key={conv.id}
                          role="option"
                          tabIndex={0}
                          aria-selected={focused}
                          onClick={() => setPreviewConversation(conv)}
                          onDoubleClick={() => void openConversation(conv)}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") {
                              event.preventDefault()
                              void openConversation(conv)
                            }
                          }}
                          className={cn(
                            "flex items-center gap-2 rounded-md px-2 py-1.5 cursor-pointer border border-transparent",
                            "hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                            checked && "border-accent/60",
                            focused && "bg-accent/50 border-primary/30"
                          )}
                        >
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation()
                              toggleOne(conv)
                            }}
                            className="flex h-5 w-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:text-foreground"
                            aria-pressed={checked}
                            aria-label={t("selectConversation", {
                              title:
                                formatConversationTitle(conv.title) ||
                                t("untitledConversation"),
                            })}
                          >
                            {checked ? (
                              <CheckSquare className="h-4 w-4 text-primary" />
                            ) : (
                              <Square className="h-4 w-4" />
                            )}
                          </button>
                          <AgentIcon
                            agentType={conv.agent_type}
                            className="h-4 w-4 shrink-0"
                          />
                          <span className="flex min-w-0 flex-1 flex-col">
                            <span className="truncate text-sm">
                              {formatConversationTitle(conv.title) ||
                                t("untitledConversation")}
                            </span>
                            {contentSnippets.has(conv.id) && (
                              <span className="flex min-w-0 items-start gap-1 text-xs text-muted-foreground">
                                <MessageSquareText className="mt-0.5 h-3 w-3 shrink-0" />
                                <span className="line-clamp-2 whitespace-pre-line">
                                  {contentSnippets.get(conv.id)}
                                </span>
                              </span>
                            )}
                          </span>
                          {collection ? (
                            <span
                              className="flex max-w-24 shrink-0 items-center gap-1 rounded-full bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
                              title={collection.name}
                            >
                              <FolderTree className="h-3 w-3 shrink-0" />
                              <span className="truncate">
                                {collection.name}
                              </span>
                            </span>
                          ) : null}
                          {refs.length > 0 ? (
                            <span
                              className="flex shrink-0 items-center gap-1 rounded-full bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
                              title={refs
                                .map((ref) => ref.workbench_name)
                                .join(", ")}
                            >
                              <PanelsTopLeft className="h-3 w-3" />
                              {refs.length}
                            </span>
                          ) : null}
                          {collaboration ? (
                            <span className="flex shrink-0 items-center gap-1 text-[10px] tabular-nums">
                              {collaboration.unreadCount > 0 ? (
                                <span
                                  className="rounded-full bg-primary/10 px-1.5 py-0.5 text-primary"
                                  title={tCollaboration("unreadCount", {
                                    count: collaboration.unreadCount,
                                  })}
                                >
                                  {collaboration.unreadCount}
                                </span>
                              ) : null}
                              {collaboration.needsReplyCount > 0 ? (
                                <span
                                  className="rounded-full bg-amber-500/10 px-1.5 py-0.5 text-amber-700 dark:text-amber-400"
                                  title={tCollaboration("stateNeedsReply")}
                                >
                                  {collaboration.needsReplyCount}
                                </span>
                              ) : null}
                              {collaboration.awaitingReplyCount > 0 ? (
                                // Amber like needsReply: one convention —
                                // amber means a reply is owed, whichever side
                                // owes it (the title names the direction).
                                <span
                                  className="rounded-full bg-amber-500/10 px-1.5 py-0.5 text-amber-700 dark:text-amber-400"
                                  title={tCollaboration("stateAwaitingReply")}
                                >
                                  {collaboration.awaitingReplyCount}
                                </span>
                              ) : null}
                              {collaboration.failedCount > 0 ? (
                                <span
                                  className="rounded-full bg-destructive/10 px-1.5 py-0.5 text-destructive"
                                  title={tCollaboration("stateFailed")}
                                >
                                  {collaboration.failedCount}
                                </span>
                              ) : null}
                            </span>
                          ) : null}
                          {showFolderColumn ? (
                            // Workspace-wide scope only: with rows from every
                            // folder the title alone doesn't say which project a
                            // conversation belongs to.
                            <span
                              className="shrink-0 max-w-28 truncate text-xs text-muted-foreground"
                              title={
                                folder
                                  ? [
                                      formatFolderLabelWithAlias(folder),
                                      folder.path,
                                    ]
                                      .filter(Boolean)
                                      .join(" · ")
                                  : `#${conv.folder_id}`
                              }
                            >
                              {folder ? (
                                <FolderAliasLabel
                                  name={folder.name}
                                  alias={folder.alias}
                                />
                              ) : (
                                `#${conv.folder_id}`
                              )}
                            </span>
                          ) : null}
                          {/* The branch the conversation was started on — what
                            tells two runs of the same project apart. */}
                          <span
                            className="flex w-28 shrink-0 items-center justify-end gap-1 text-xs text-muted-foreground"
                            title={conv.git_branch ?? t("branchNone")}
                          >
                            {conv.git_branch ? (
                              <>
                                <GitBranch
                                  className="h-3 w-3 shrink-0"
                                  aria-hidden="true"
                                />
                                <span dir="ltr" className="truncate">
                                  {conv.git_branch}
                                </span>
                              </>
                            ) : (
                              <span className="text-muted-foreground/60">
                                —
                              </span>
                            )}
                          </span>
                          <span className="shrink-0 text-xs text-muted-foreground w-10 text-right">
                            {formatRelative(conv.created_at)}
                          </span>
                          <ConversationStatusDot
                            status={conv.status as ConversationStatus}
                            title={
                              STATUS_ORDER.includes(
                                conv.status as ConversationStatus
                              )
                                ? tStatus(conv.status as ConversationStatus)
                                : conv.status
                            }
                          />
                        </div>
                      )
                    })
                  )}
                </div>
              </ScrollArea>
            </div>

            {/* A management preview, deliberately separate from the live
                conversation runtime. Selecting a row cannot start an agent or
                mutate the current layout. */}
            <section
              aria-label={t("previewTitle")}
              className={cn(
                "min-h-0 min-w-0 flex-col overflow-hidden rounded-md border border-border/50 bg-muted/10",
                previewConversation ? "flex" : "hidden md:flex"
              )}
            >
              {!previewConversation ? (
                <div className="flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center text-sm text-muted-foreground">
                  <MessageSquareText className="h-7 w-7 opacity-40" />
                  <p>{t("previewEmpty")}</p>
                  <p className="text-xs opacity-75">{t("previewReadOnly")}</p>
                </div>
              ) : (
                <>
                  <div className="space-y-2 border-b border-border/50 p-3">
                    <div className="flex min-w-0 items-start gap-2">
                      <Button
                        type="button"
                        size="icon-sm"
                        variant="ghost"
                        className="-ms-1 shrink-0 md:hidden"
                        aria-label={t("backToSessionList")}
                        onClick={() => setPreviewConversation(null)}
                      >
                        <ArrowLeft className="h-4 w-4 rtl:rotate-180" />
                      </Button>
                      <AgentIcon
                        agentType={previewConversation.agent_type}
                        className="mt-0.5 h-5 w-5 shrink-0"
                      />
                      <div className="min-w-0 flex-1">
                        <h3 className="truncate text-sm font-medium">
                          {formatConversationTitle(previewConversation.title) ||
                            t("untitledConversation")}
                        </h3>
                        <p className="truncate text-xs text-muted-foreground">
                          {getAgentLabel(previewConversation.agent_type)}
                          {previewConversation.model
                            ? ` · ${previewConversation.model}`
                            : ""}
                        </p>
                      </div>
                      <ConversationStatusDot
                        status={
                          previewConversation.status as ConversationStatus
                        }
                        title={
                          STATUS_ORDER.includes(
                            previewConversation.status as ConversationStatus
                          )
                            ? tStatus(
                                previewConversation.status as ConversationStatus
                              )
                            : previewConversation.status
                        }
                      />
                    </div>

                    <div className="flex flex-wrap gap-1.5 text-xs text-muted-foreground">
                      <span
                        className="max-w-full truncate rounded-full bg-muted px-2 py-0.5"
                        title={previewFolder?.path ?? undefined}
                      >
                        {previewFolder
                          ? formatFolderLabelWithAlias(previewFolder)
                          : `#${previewConversation.folder_id}`}
                      </span>
                      {previewCollection ? (
                        <span className="flex max-w-full items-center gap-1 rounded-full bg-muted px-2 py-0.5">
                          <FolderTree className="h-3 w-3 shrink-0" />
                          <span className="truncate">
                            {previewCollection.name}
                          </span>
                        </span>
                      ) : null}
                      {previewConversation.git_branch ? (
                        <span className="flex max-w-full items-center gap-1 rounded-full bg-muted px-2 py-0.5">
                          <GitBranch className="h-3 w-3 shrink-0" />
                          <span dir="ltr" className="truncate">
                            {previewConversation.git_branch}
                          </span>
                        </span>
                      ) : null}
                      <span className="rounded-full bg-muted px-2 py-0.5">
                        {t("messageCount", {
                          count: previewConversation.message_count,
                        })}
                      </span>
                    </div>

                    <div className="space-y-1">
                      <p className="text-xs font-medium">
                        {t("openedInWorkbenches")}
                      </p>
                      <div className="flex flex-wrap gap-1.5">
                        {previewWorkbenchRefs.length > 0 ? (
                          previewWorkbenchRefs.map((ref) => (
                            <Button
                              key={ref.workbench_id}
                              type="button"
                              size="sm"
                              variant={
                                ref.workbench_id === activeWorkbenchId
                                  ? "secondary"
                                  : "outline"
                              }
                              disabled={openingWorkbenchId !== null}
                              className="h-7 max-w-full gap-1 px-2 text-xs"
                              onClick={() =>
                                void openConversation(
                                  previewConversation,
                                  ref.workbench_id
                                )
                              }
                            >
                              {openingWorkbenchId === ref.workbench_id ? (
                                <Loader2 className="h-3 w-3 animate-spin" />
                              ) : (
                                <PanelsTopLeft className="h-3 w-3" />
                              )}
                              <span className="truncate">
                                {ref.workbench_name}
                              </span>
                              {ref.workbench_id === activeWorkbenchId ? (
                                <span className="opacity-60">
                                  · {t("currentWorkbench")}
                                </span>
                              ) : null}
                            </Button>
                          ))
                        ) : workbenchRefsUnavailable ? (
                          <span className="text-xs text-muted-foreground">
                            {t("workbenchOwnershipUnavailable")}
                          </span>
                        ) : (
                          <span className="text-xs text-muted-foreground">
                            {t("notOpenInWorkbench")}
                          </span>
                        )}
                      </div>
                    </div>
                  </div>

                  <div className="flex min-h-0 flex-1 flex-col">
                    <p className="border-b border-border/40 px-3 py-2 text-xs font-medium">
                      {t("recentMessages")}
                    </p>
                    <ScrollArea className="min-h-0 flex-1">
                      <div className="space-y-2 p-3">
                        {previewLoading ? (
                          Array.from({ length: 4 }).map((_, index) => (
                            <Skeleton
                              key={index}
                              className="h-12 w-full rounded-md"
                            />
                          ))
                        ) : previewError ? (
                          <p className="px-2 py-4 text-center text-xs text-destructive">
                            {t("previewFailed", { message: previewError })}
                          </p>
                        ) : previewMessages.length === 0 ? (
                          <p className="px-2 py-4 text-center text-xs text-muted-foreground">
                            {t("noReadableMessages")}
                          </p>
                        ) : (
                          previewMessages.map(({ turn, text }) => (
                            <article
                              key={turn.id}
                              className="rounded-md border border-border/40 bg-background/70 p-2"
                            >
                              <header className="mb-1 flex items-center justify-between gap-2 text-[10px] text-muted-foreground">
                                <span className="font-medium uppercase tracking-wide">
                                  {turn.role === "user"
                                    ? t("roleUser")
                                    : turn.role === "assistant"
                                      ? t("roleAssistant")
                                      : t("roleSystem")}
                                </span>
                                <span>{formatRelative(turn.timestamp)}</span>
                              </header>
                              <p className="line-clamp-4 whitespace-pre-wrap break-words text-xs leading-relaxed">
                                {text}
                              </p>
                            </article>
                          ))
                        )}
                      </div>
                    </ScrollArea>
                  </div>
                </>
              )}
            </section>
          </div>

          {/* Footer: bulk actions */}
          <DialogFooter className="flex shrink-0 flex-col items-stretch justify-start gap-2 sm:flex-col sm:items-stretch sm:justify-start md:flex-row md:items-center md:justify-between">
            <span className="w-full text-xs text-muted-foreground md:w-auto">
              {t("selectedCount", { count: selectedCount })}
            </span>
            <div className="grid w-full grid-cols-2 gap-2 md:flex md:w-auto md:flex-wrap md:items-center">
              <div className="col-span-2 flex min-w-0 md:col-span-1">
                <Button
                  size="sm"
                  variant="default"
                  disabled={
                    selectedCount === 0 ||
                    pending ||
                    bulkOpening ||
                    openingWorkbenchId !== null
                  }
                  className="min-w-0 flex-1 rounded-r-none"
                  onClick={() => void handleAddSelectedToWorkbench("current")}
                >
                  {bulkOpening ? (
                    <Loader2 className="mr-1 h-3.5 w-3.5 shrink-0 animate-spin" />
                  ) : (
                    <PanelsTopLeft className="mr-1 h-3.5 w-3.5 shrink-0" />
                  )}
                  <span className="truncate">
                    {t("addSelectedToWorkbench", {
                      workbench: activeWorkbenchName,
                      count: selectedCount,
                    })}
                  </span>
                </Button>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      size="sm"
                      variant="default"
                      disabled={
                        selectedCount === 0 ||
                        pending ||
                        bulkOpening ||
                        openingWorkbenchId !== null
                      }
                      className="rounded-l-none border-l border-primary-foreground/25 px-2"
                      aria-label={t("chooseAddTarget")}
                    >
                      <ChevronDown className="h-3.5 w-3.5" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="min-w-56">
                    {selectedCount === 1 ? (
                      <>
                        <DropdownMenuItem
                          onSelect={() =>
                            void handleAddSelectedToWorkbench("current")
                          }
                        >
                          <PanelTopOpen className="h-4 w-4" />
                          {t("openInCurrentPane")}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          onSelect={() =>
                            void handleAddSelectedToWorkbench(
                              "current",
                              "right"
                            )
                          }
                        >
                          <PanelRightOpen className="h-4 w-4" />
                          {t("openRight")}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          onSelect={() =>
                            void handleAddSelectedToWorkbench("current", "down")
                          }
                        >
                          <PanelBottomOpen className="h-4 w-4" />
                          {t("openDown")}
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                      </>
                    ) : null}
                    {workbenches.map((workbench) => (
                      <DropdownMenuItem
                        key={workbench.id}
                        onSelect={() =>
                          void handleAddSelectedToWorkbench(
                            workbench.id === activeWorkbenchId
                              ? "current"
                              : workbench.id
                          )
                        }
                      >
                        <PanelsTopLeft className="h-4 w-4" />
                        <span className="truncate">{workbench.name}</span>
                        {workbench.id === activeWorkbenchId ? (
                          <span className="opacity-60">
                            · {t("currentWorkbench")}
                          </span>
                        ) : null}
                      </DropdownMenuItem>
                    ))}
                    <DropdownMenuSeparator />
                    <DropdownMenuItem
                      onSelect={() => void handleAddSelectedToWorkbench("new")}
                    >
                      <Plus className="h-4 w-4" />
                      {t("createNewWorkbench")}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>

              {/* Set status */}
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={selectedCount === 0 || pending}
                  >
                    <ListChecks className="h-3.5 w-3.5 mr-1" />
                    {t("setStatus")}
                    <ChevronDown className="h-3 w-3 ml-1 opacity-60" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  {STATUS_ORDER.map((s) => (
                    <DropdownMenuItem
                      key={s}
                      onSelect={() => handleBulkStatus(s)}
                    >
                      <ConversationStatusDot status={s} />
                      {tStatus(s)}
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>

              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={
                      selectedCount === 0 ||
                      pending ||
                      collectionRefsUnavailable
                    }
                    title={
                      collectionRefsUnavailable
                        ? t("collectionUnavailable")
                        : undefined
                    }
                  >
                    <FolderTree className="mr-1 h-3.5 w-3.5" />
                    {t("moveToCollection")}
                    <ChevronDown className="ml-1 h-3 w-3 opacity-60" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent
                  align="end"
                  className="max-h-72 w-64 overflow-y-auto"
                >
                  <DropdownMenuItem
                    onSelect={() => void handleBulkCollection(null)}
                  >
                    <FolderTree className="h-3.5 w-3.5 opacity-50" />
                    {t("collectionUnclassified")}
                  </DropdownMenuItem>
                  {collectionOptions.map(({ item, depth, path }) => (
                    <DropdownMenuItem
                      key={item.id}
                      title={path}
                      onSelect={() => void handleBulkCollection(item.id)}
                    >
                      <FolderTree className="h-3.5 w-3.5" />
                      <span className="truncate">
                        {depth > 0 ? `${"· ".repeat(depth)}` : ""}
                        {item.name}
                      </span>
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>

              <Button
                size="sm"
                variant="outline"
                disabled={selectedCount === 0 || pending}
                onClick={() =>
                  void handleBulkArchive(statusFilter !== "archived")
                }
              >
                {statusFilter === "archived" ? (
                  <ArchiveRestore className="mr-1 h-3.5 w-3.5" />
                ) : (
                  <Archive className="mr-1 h-3.5 w-3.5" />
                )}
                {statusFilter === "archived"
                  ? t("restoreSelected")
                  : t("archiveSelected")}
              </Button>

              {/* Delete */}
              <Button
                size="sm"
                variant="destructive"
                disabled={selectedCount === 0 || pending}
                onClick={() => setConfirmDelete(true)}
              >
                {pending ? (
                  <Loader2 className="h-3.5 w-3.5 mr-1 animate-spin" />
                ) : (
                  <Trash2 className="h-3.5 w-3.5 mr-1" />
                )}
                {t("deleteSelected")}
              </Button>

              <Button
                size="sm"
                variant="outline"
                className="hidden md:inline-flex"
                onClick={() => onOpenChange(false)}
              >
                {tCommon("close")}
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete confirmation */}
      <AlertDialog
        open={confirmDelete}
        onOpenChange={(o) => !o && setConfirmDelete(false)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t("confirmDeleteTitle", { count: selectedCount })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t("confirmDeleteDescription")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{tCommon("cancel")}</AlertDialogCancel>
            <AlertDialogAction onClick={handleBulkDelete}>
              {tCommon("confirm")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
