"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  pointerWithin,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragOverEvent,
  type DragStartEvent,
} from "@dnd-kit/core"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import {
  ArrowDownWideNarrow,
  Circle,
  CircleCheckBig,
  CircleDashed,
  CircleGauge,
  CircleSlash2,
  CircleX,
  Clock3,
  ListTodo,
  Plus,
  SlidersHorizontal,
  Tag,
  UserRound,
  type LucideIcon,
} from "lucide-react"
import { useTasksView } from "@/contexts/tasks-view-context"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import {
  workTaskArchive,
  workTaskAssignSession,
  workTaskCreate,
  workTaskMergeUnqueue,
  workTaskRequestReview,
  workTaskSetManualStatus,
  workTaskStart,
  workTaskUpdate,
} from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import {
  consumePendingTaskDetail,
  consumePendingTaskDraft,
  CREATE_TASK_FROM_TEXT_EVENT,
  OPEN_TASK_DETAIL_EVENT,
  type CreateTaskFromTextDetail,
} from "@/lib/task-compose-events"
import { getAgentLabel } from "@/lib/custom-agents"
import {
  loadTasksBoardFilter,
  loadTasksBoardGrouping,
  loadTasksOwnerFilter,
  loadTasksScope,
  loadTasksSort,
  loadTasksStatusFilter,
  saveTasksBoardFilter,
  saveTasksBoardGrouping,
  saveTasksOwnerFilter,
  saveTasksScope,
  saveTasksSort,
  saveTasksStatusFilter,
  TASKS_BOARD_GROUPINGS,
  TASKS_SCOPES,
  TASKS_SORTS,
  type TasksBoardGrouping,
  type TasksScope,
  type TasksSort,
} from "@/lib/tasks-board-filter-storage"
import { WorkbenchPageTitle } from "@/components/workbench/workbench-page-title"
import { FolderSelect } from "@/components/shared/folder-select"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { ScrollArea } from "@/components/ui/scroll-area"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { cn } from "@/lib/utils"
import {
  BOARD_COLUMN_IDS,
  columnForStatus,
  filterTasksForList,
  groupTasksByColumn,
  statusForColumn,
  type BoardColumnId,
} from "./board-columns"
import { groupingShowsHeaders, segmentTasksForGrouping } from "./board-grouping"
import { filterTasksByScope } from "./task-scope"
import {
  connectionKeysForTask,
  taskActivityDot,
  useTaskConnectionsByConversationId,
  type TaskActivityDot,
  type TaskConnectionLookup,
} from "./task-activity"
import { TaskGroupHeader } from "./task-group-header"
import {
  isFolderMerging,
  isMergeQueued,
  mergeQueueRanks,
} from "./task-acceptance"
import { TaskCancelDialog } from "./task-cancel-dialog"
import { TaskAssignSessionDialog } from "./task-assign-session-dialog"
import { StatusChip, TaskCard } from "./task-card"
import type { TaskActionHandlers } from "./task-actions"
import { TaskCompleteDialog } from "./task-complete-dialog"
import { TaskDetailSheet } from "./task-detail-sheet"
import { TaskEditorDialog } from "./task-editor-dialog"
import { TaskList } from "./task-list"
import { TaskMergeDialog } from "./task-merge-dialog"
import { TaskRestartDialog, type TaskRestartKind } from "./task-restart-dialog"
import { TaskScheduleDialog } from "./task-schedule-dialog"
import { TaskSettingsDialog } from "./task-settings-dialog"
import { OPEN_TASK_SETTINGS_EVENT } from "./tasks-chrome-actions"
import { TasksSkeleton } from "./tasks-skeleton"
import { TaskTranscriptDialog } from "./task-transcript-dialog"
import { TaskSessionLaunchDialog } from "./task-session-launch-dialog"
import { createTaskSessionAndAssign } from "./task-session-launch"
import { taskPriorityRank } from "./task-priority"
import type {
  DbConversationSummary,
  WorkTask,
  WorkTaskBusinessStatus,
  WorkTaskDraft,
} from "@/lib/types"

const COLUMN_LABEL_KEYS = {
  backlog: "colBacklog",
  todo: "colTodo",
  inProgress: "colInProgress",
  review: "colReview",
  done: "colDone",
  blocked: "colBlocked",
  canceled: "colCanceled",
} as const satisfies Record<BoardColumnId, string>

const EMPTY_LABEL_KEYS = {
  backlog: "emptyColBacklog",
  todo: "emptyColTodo",
  inProgress: "emptyColInProgress",
  review: "emptyColReview",
  done: "emptyColDone",
  blocked: "emptyColBlocked",
  canceled: "emptyColCanceled",
} as const satisfies Record<BoardColumnId, string>

/** The status select's "no filter" option — a sentinel, because Radix reserves
 *  the empty string for "no value chosen". */
const ALL_STATUSES = "__all__"

const GROUPING_LABEL_KEYS = {
  none: "groupByNone",
  folder: "groupByFolder",
  session: "groupBySession",
  agent: "groupByAgent",
} as const satisfies Record<TasksBoardGrouping, string>

const SCOPE_LABEL_KEYS = {
  all: "scopeAll",
  agent: "scopeAgent",
  attention: "scopeAttention",
} as const satisfies Record<TasksScope, string>

const SORT_LABEL_KEYS = {
  manual: "sortManual",
  priority: "sortPriority",
  updated: "sortUpdated",
} as const satisfies Record<TasksSort, string>

function compareTasks(a: WorkTask, b: WorkTask, sort: TasksSort): number {
  if (sort === "priority") {
    const byPriority =
      taskPriorityRank(b.priority) - taskPriorityRank(a.priority)
    if (byPriority !== 0) return byPriority
  } else if (sort === "updated") {
    const byUpdated = b.updated_at.localeCompare(a.updated_at)
    if (byUpdated !== 0) return byUpdated
  }
  return a.sort_order - b.sort_order || a.id - b.id
}

/** Multica-style status glyphs, shared by headers and every status picker. */
const COLUMN_ICONS = {
  backlog: CircleDashed,
  todo: Circle,
  inProgress: CircleGauge,
  review: Clock3,
  done: CircleCheckBig,
  blocked: CircleSlash2,
  canceled: CircleX,
} as const satisfies Record<BoardColumnId, LucideIcon>

const COLUMN_ICON_CLASSES = {
  backlog: "text-muted-foreground/70",
  todo: "text-muted-foreground",
  inProgress: "text-amber-500",
  review: "text-emerald-600 dark:text-emerald-400",
  done: "text-blue-600 dark:text-blue-400",
  blocked: "text-rose-500",
  canceled: "text-muted-foreground/70",
} as const satisfies Record<BoardColumnId, string>

function TaskColumnIcon({
  column,
  className,
}: {
  column: BoardColumnId
  className?: string
}) {
  const Icon = COLUMN_ICONS[column]
  return (
    <Icon
      className={cn("size-4 shrink-0", COLUMN_ICON_CLASSES[column], className)}
      aria-hidden="true"
    />
  )
}

/** The cards inside a column. gap-4 — the same gutter the columns keep from
 *  each other and from the window edge — so a card is inset by 1rem on all
 *  four sides instead of being packed tighter vertically than horizontally.
 *  pb-1 keeps the last card clear of the scroll area's edge. */
const CARD_LIST_CLASS = "flex flex-col gap-4 pb-1"

const isBoardDraggable = (task: WorkTask) =>
  task.archived_at == null &&
  (task.execution_mode == null || task.execution_mode === "manual")

const taskDragId = (taskId: number) => `task:${taskId}`
const columnDropId = (column: BoardColumnId) => `column:${column}`

const taskIdFromDragId = (id: string | number): number | null => {
  const match = /^task:(\d+)$/.exec(String(id))
  return match ? Number(match[1]) : null
}

const columnFromDropId = (
  id: string | number | null | undefined
): BoardColumnId | null => {
  const value = String(id ?? "").replace(/^column:/, "")
  return BOARD_COLUMN_IDS.includes(value as BoardColumnId)
    ? (value as BoardColumnId)
    : null
}

/** Page title rendered into the window-chrome strip above the page (the h-10
 *  band shared with the fixed corner overlays) — the shared breadcrumb header,
 *  nothing else. The page-level controls (view switch, task settings) sit in
 *  the window's top-right chrome cluster instead, next to the settings gear —
 *  see TasksChromeActions. */
export function TasksPageTitle() {
  const t = useTranslations("Tasks")
  return <WorkbenchPageTitle title={t("title")} />
}

/**
 * The Tasks route page: toolbar (folder scope, stable views, display settings,
 * owner/status filters, new task) plus the board or the list. The page title
 * lives in the chrome strip above (TasksPageTitle); the view switch + settings
 * entry live in the window's top-right cluster (TasksChromeActions), so the
 * toolbar is purely "narrow what you see" + "add one".
 * Data comes from the always-mounted TasksViewProvider;
 * every mutation is fire-and-refetch — the engine's `task://changed` nudges
 * keep all clients converged.
 */
export function TasksPage() {
  const t = useTranslations("Tasks")
  const { tasks, loading, refetch, viewMode } = useTasksView()
  const folders = useAppWorkspaceStore((s) => s.folders)
  const allFolders = useAppWorkspaceStore((s) => s.allFolders)
  const conversations = useAppWorkspaceStore((s) => s.conversations)
  const projectFolders = useMemo(
    () => folders.filter((f) => f.parent_id == null && f.kind === "regular"),
    [folders]
  )
  const folderNames = useMemo(() => {
    const map = new Map<number, string>()
    for (const f of folders) map.set(f.id, f.alias ?? f.name)
    return map
  }, [folders])
  const conversationNames = useMemo(() => {
    const map = new Map<number, string>()
    for (const conversation of conversations) {
      map.set(
        conversation.id,
        conversation.title?.trim() ||
          `${getAgentLabel(conversation.agent_type)} #${conversation.id}`
      )
    }
    return map
  }, [conversations])

  const [selectedFolderFilter, setFolderFilter] = useState<number | null>(null)
  // A folder can leave the workspace (closed, or removed) while it is the active
  // filter. Fall back to "all" by derivation — no effect, the same guard the
  // Automations list uses — so the board, the drag scope, the new-task prefill,
  // the settings scope and the pill can never disagree about which folder is in
  // effect: without it the pill would read "all folders" while everything below
  // it still scoped to the folder that vanished.
  const folderFilter =
    selectedFolderFilter != null &&
    !projectFolders.some((f) => f.id === selectedFolderFilter)
      ? null
      : selectedFolderFilter
  // Restored synchronously from localStorage: this page mounts only after a
  // client-side route switch (never prerendered), so there is no SSR markup to
  // mismatch — and the board paints with the remembered filter right away.
  const [boardFilter, setBoardFilter] = useState(loadTasksBoardFilter)
  useEffect(() => {
    saveTasksBoardFilter(boardFilter)
  }, [boardFilter])
  // The list's own status selection — one board COLUMN, or `null` for every
  // status. Restored synchronously for the same reason as the filter above.
  // `viewMode` itself lives in TasksViewProvider — its switch renders in the
  // window's chrome cluster.
  const [statusFilter, setStatusFilter] = useState<BoardColumnId | null>(
    loadTasksStatusFilter
  )
  useEffect(() => {
    saveTasksStatusFilter(statusFilter)
  }, [statusFilter])
  // Column-internal grouping (none / by project / by agent). Same restore
  // pattern as viewMode: this page never SSR-paints, so a synchronous
  // localStorage read is safe and the board opens already grouped.
  const [grouping, setGrouping] = useState<TasksBoardGrouping>(
    loadTasksBoardGrouping
  )
  useEffect(() => {
    saveTasksBoardGrouping(grouping)
  }, [grouping])
  const [scope, setScope] = useState<TasksScope>(loadTasksScope)
  useEffect(() => {
    saveTasksScope(scope)
  }, [scope])
  const [sort, setSort] = useState<TasksSort>(loadTasksSort)
  useEffect(() => {
    saveTasksSort(sort)
  }, [sort])
  const [selectedOwnerFilter, setOwnerFilter] = useState<number | null>(
    loadTasksOwnerFilter
  )
  // A drop acts on the LIVE row, not the snapshot the drag started from: the
  // provider refetches throughout a drag, and the engine's auto-processor can
  // claim a pending task while it is in the air.
  const tasksRef = useRef(tasks)
  tasksRef.current = tasks
  const [drag, setDrag] = useState<{ task: WorkTask } | null>(null)
  const [dropTarget, setDropTarget] = useState<BoardColumnId | null>(null)
  // A drag ends with a click on the card underneath; without this latch the
  // detail sheet would open on top of whatever the drop just did.
  const draggedRef = useRef(false)
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } })
  )
  const [editorOpen, setEditorOpen] = useState(false)
  const [editorTask, setEditorTask] = useState<WorkTask | null>(null)
  const [editorInitialStatus, setEditorInitialStatus] =
    useState<WorkTaskBusinessStatus>("todo")
  const [editorPrefill, setEditorPrefill] =
    useState<CreateTaskFromTextDetail | null>(null)

  // One shared timestamp per render tick keeps every card's relative-time
  // label consistent; a minute interval bounds staleness.
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 60_000)
    return () => window.clearInterval(id)
  }, [])

  // "Task from message" hand-off: consume the parked draft on mount (this page
  // is unmounted while other routes are active) and on the live event.
  useEffect(() => {
    const consume = () => {
      const draft = consumePendingTaskDraft()
      if (!draft) return
      setEditorTask(null)
      setEditorInitialStatus("todo")
      setEditorPrefill(draft)
      setEditorOpen(true)
    }
    consume()
    window.addEventListener(CREATE_TASK_FROM_TEXT_EVENT, consume)
    return () =>
      window.removeEventListener(CREATE_TASK_FROM_TEXT_EVENT, consume)
  }, [])
  const [detailTaskId, setDetailTaskId] = useState<number | null>(null)

  // Session details and other projections can point at a task while this route
  // is unmounted. Consume the parked id on mount; the event is the already-open
  // fast path. The detail sheet still reads the live row from TasksViewProvider.
  useEffect(() => {
    const consume = () => {
      const taskId = consumePendingTaskDetail()
      if (taskId != null) setDetailTaskId(taskId)
    }
    consume()
    window.addEventListener(OPEN_TASK_DETAIL_EVENT, consume)
    return () => window.removeEventListener(OPEN_TASK_DETAIL_EVENT, consume)
  }, [])
  const [mergeTask, setMergeTask] = useState<WorkTask | null>(null)
  const [mergeOpen, setMergeOpen] = useState(false)
  // The merge dialog's counterpart for a task that changed nothing.
  const [completeTask, setCompleteTask] = useState<WorkTask | null>(null)
  const [completeOpen, setCompleteOpen] = useState(false)
  // Stopping a task asks why (optional) — from the card and from the drawer.
  const [cancelTask, setCancelTask] = useState<WorkTask | null>(null)
  const [cancelOpen, setCancelOpen] = useState(false)
  // Restarting from a card asks for an optional note; the drawer unfolds its
  // own box in place instead.
  const [restartTask, setRestartTask] = useState<WorkTask | null>(null)
  const [restartKind, setRestartKind] = useState<TaskRestartKind>("retry")
  const [restartOpen, setRestartOpen] = useState(false)
  // Planning a to-do task's start — tracked by id so the dialog reads the live
  // row (a task claimed while it is open no longer accepts a plan).
  const [scheduleTaskId, setScheduleTaskId] = useState<number | null>(null)
  const [scheduleOpen, setScheduleOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [assignTaskId, setAssignTaskId] = useState<number | null>(null)
  const [assignOpen, setAssignOpen] = useState(false)
  const [launchTaskId, setLaunchTaskId] = useState<number | null>(null)
  const [launchOpen, setLaunchOpen] = useState(false)
  // Read-only live session viewer ("查看会话") — tracked by id so the dialog
  // header's status chip follows the live row (like the detail sheet).
  const [sessionTaskId, setSessionTaskId] = useState<number | null>(null)
  const [sessionOpen, setSessionOpen] = useState(false)

  // The settings entry lives in the chrome strip (TasksPageTitle), a separate
  // tree branch — it asks this page (which owns the dialog + folder scope) to
  // open via a window event, like the "task from message" hand-off above.
  useEffect(() => {
    const open = () => setSettingsOpen(true)
    window.addEventListener(OPEN_TASK_SETTINGS_EVENT, open)
    return () => window.removeEventListener(OPEN_TASK_SETTINGS_EVENT, open)
  }, [])

  const folderScopedTasks = useMemo(
    () =>
      folderFilter == null
        ? tasks
        : tasks.filter((task) => task.folder_id === folderFilter),
    [tasks, folderFilter]
  )
  const attentionCount = useMemo(
    () =>
      folderScopedTasks.filter(
        (task) =>
          task.archived_at == null &&
          (task.task_status === "review" || task.task_status === "blocked")
      ).length,
    [folderScopedTasks]
  )
  const ownerOptions = useMemo(() => {
    const ids = new Set<number>()
    for (const task of folderScopedTasks) {
      if (task.conversation_id != null) ids.add(task.conversation_id)
    }
    return [...ids]
      .map((id) => ({
        id,
        label: conversationNames.get(id) ?? `Session #${id}`,
      }))
      .sort((a, b) => a.label.localeCompare(b.label))
  }, [folderScopedTasks, conversationNames])
  const ownerFilter =
    selectedOwnerFilter != null &&
    !ownerOptions.some((option) => option.id === selectedOwnerFilter)
      ? null
      : selectedOwnerFilter
  useEffect(() => {
    saveTasksOwnerFilter(ownerFilter)
  }, [ownerFilter])
  const visibleTasks = useMemo(() => {
    const scoped = filterTasksByScope(folderScopedTasks, scope)
    return ownerFilter == null
      ? scoped
      : scoped.filter((task) => task.conversation_id === ownerFilter)
  }, [folderScopedTasks, scope, ownerFilter])
  const columns = useMemo(() => {
    const grouped = groupTasksByColumn(visibleTasks, boardFilter.showArchived)
    for (const column of BOARD_COLUMN_IDS) {
      grouped[column].sort((a, b) => compareTasks(a, b, sort))
    }
    return grouped
  }, [visibleTasks, boardFilter, sort])
  // The list view's rows: one flat, freshest-first sequence, narrowed to a
  // single board column. The visibility toggles apply here exactly as they do
  // on the board — one pair of controls for both views.
  const listTasks = useMemo(
    () =>
      filterTasksForList(
        visibleTasks,
        statusFilter,
        boardFilter.showArchived
      ).sort((a, b) => compareTasks(a, b, sort)),
    [visibleTasks, statusFilter, boardFilter, sort]
  )

  const showGroupHeaders = groupingShowsHeaders(grouping, folderFilter)
  const groupingOpts = useMemo(
    () => ({
      folderNames,
      sessionNames: conversationNames,
      folderFilter,
      agentLabel: getAgentLabel,
    }),
    [folderNames, conversationNames, folderFilter]
  )
  const columnSegments = useMemo(
    () => ({
      backlog: segmentTasksForGrouping(columns.backlog, grouping, groupingOpts),
      todo: segmentTasksForGrouping(columns.todo, grouping, groupingOpts),
      inProgress: segmentTasksForGrouping(
        columns.inProgress,
        grouping,
        groupingOpts
      ),
      review: segmentTasksForGrouping(columns.review, grouping, groupingOpts),
      done: segmentTasksForGrouping(columns.done, grouping, groupingOpts),
      blocked: segmentTasksForGrouping(columns.blocked, grouping, groupingOpts),
      canceled: segmentTasksForGrouping(
        columns.canceled,
        grouping,
        groupingOpts
      ),
    }),
    [columns, grouping, groupingOpts]
  )
  const listSegments = useMemo(
    () => segmentTasksForGrouping(listTasks, grouping, groupingOpts),
    [listTasks, grouping, groupingOpts]
  )
  const connectionLookups = useMemo(() => {
    const seen = new Set<number>()
    const lookups: TaskConnectionLookup[] = []
    for (const task of visibleTasks) {
      if (task.status !== "running" || task.conversation_id == null) continue
      if (seen.has(task.conversation_id)) continue
      seen.add(task.conversation_id)
      const keys = connectionKeysForTask(task)
      if (keys.length === 0) continue
      lookups.push({ conversationId: task.conversation_id, keys })
    }
    return lookups
  }, [visibleTasks])
  const connectionsByConversationId =
    useTaskConnectionsByConversationId(connectionLookups)
  const activityFor = useCallback(
    (task: WorkTask): TaskActivityDot | null =>
      taskActivityDot(
        task,
        connectionsByConversationId.get(task.conversation_id ?? -1)?.status
      ),
    [connectionsByConversationId]
  )
  // Place in line for every task waiting on its project's merge slot. Computed
  // over ALL tasks, not the visible ones: the queue is the project's, and a
  // filtered board must not renumber it.
  const queueRanks = useMemo(() => mergeQueueRanks(tasks), [tasks])
  // The LIVE row behind the open merge dialog — read for its queue state only.
  // The dialog itself keeps the captured `mergeTask`: it re-seeds its form
  // whenever that object changes, and the provider hands out a fresh array on
  // every refetch, so passing the live row would wipe a half-typed commit
  // message the moment any task anywhere changed.
  const mergeLiveTask = useMemo(
    () =>
      mergeTask == null
        ? null
        : (tasks.find((task) => task.id === mergeTask.id) ?? null),
    [tasks, mergeTask]
  )
  // The sheet renders the LIVE row from the provider so status flips (e.g.
  // merging → done) update in place while it is open.
  const detailTask = useMemo(
    () => tasks.find((task) => task.id === detailTaskId) ?? null,
    [tasks, detailTaskId]
  )
  const sessionTask = useMemo(
    () => tasks.find((task) => task.id === sessionTaskId) ?? null,
    [tasks, sessionTaskId]
  )
  const scheduleTask = useMemo(
    () => tasks.find((task) => task.id === scheduleTaskId) ?? null,
    [tasks, scheduleTaskId]
  )
  const assignTask = useMemo(
    () => tasks.find((task) => task.id === assignTaskId) ?? null,
    [tasks, assignTaskId]
  )
  const launchTask = useMemo(
    () => tasks.find((task) => task.id === launchTaskId) ?? null,
    [tasks, launchTaskId]
  )
  const assignableSessions = useMemo<DbConversationSummary[]>(() => {
    if (!assignTask) return []
    const eligibleFolderIds = new Set<number>([assignTask.folder_id])
    for (const folder of allFolders) {
      if (folder.parent_id === assignTask.folder_id) {
        eligibleFolderIds.add(folder.id)
      }
    }
    return conversations
      .filter(
        (session) =>
          eligibleFolderIds.has(session.folder_id) &&
          session.kind === "regular" &&
          session.archived_at == null &&
          session.harness_internal !== true
      )
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
  }, [allFolders, assignTask, conversations])

  const act = useCallback(
    async (fn: () => Promise<unknown>) => {
      try {
        await fn()
      } catch (e) {
        toast.error(toErrorMessage(e))
      } finally {
        void refetch()
      }
    },
    [refetch]
  )

  const openSession = useCallback((task: WorkTask) => {
    if (task.conversation_id == null) return
    setSessionTaskId(task.id)
    setSessionOpen(true)
  }, [])

  const submitEditor = useCallback(
    async (draft: WorkTaskDraft) => {
      if (editorTask) await workTaskUpdate(editorTask.id, draft)
      else await workTaskCreate(draft)
      setEditorOpen(false)
      setEditorPrefill(null)
      void refetch()
    },
    [editorTask, refetch]
  )

  const openMerge = useCallback((task: WorkTask) => {
    setMergeTask(task)
    setMergeOpen(true)
  }, [])

  const openComplete = useCallback((task: WorkTask) => {
    setCompleteTask(task)
    setCompleteOpen(true)
  }, [])

  const openCancel = useCallback((task: WorkTask) => {
    setCancelTask(task)
    setCancelOpen(true)
  }, [])

  const openRestart = useCallback((task: WorkTask, kind: TaskRestartKind) => {
    setRestartTask(task)
    setRestartKind(kind)
    setRestartOpen(true)
  }, [])

  const openSchedule = useCallback((task: WorkTask) => {
    setScheduleTaskId(task.id)
    setScheduleOpen(true)
  }, [])

  const openAssignSession = useCallback((task: WorkTask) => {
    setAssignTaskId(task.id)
    setAssignOpen(true)
  }, [])

  const openNewSession = useCallback((task: WorkTask) => {
    setLaunchTaskId(task.id)
    setLaunchOpen(true)
  }, [])

  const openNewTask = useCallback(() => {
    setEditorTask(null)
    setEditorInitialStatus("todo")
    setEditorOpen(true)
  }, [])

  const openNewTaskInColumn = useCallback((column: BoardColumnId) => {
    setEditorTask(null)
    setEditorPrefill(null)
    setEditorInitialStatus(statusForColumn(column))
    setEditorOpen(true)
  }, [])

  // One handler set, wired the same way for a board card and a list row.
  const handlersFor = useCallback(
    (task: WorkTask): TaskActionHandlers => ({
      onManualStatus: (to) =>
        void act(() => workTaskSetManualStatus(task.id, task.task_status, to)),
      onStart: () => {
        if (task.execution_mode === "engine") {
          void act(() => workTaskStart(task.id))
        } else {
          openNewSession(task)
        }
      },
      onAssignSession: () => openAssignSession(task),
      onCancel: () => openCancel(task),
      onSubmitReview: () => void act(() => workTaskRequestReview(task.id)),
      onRetry: () => openRestart(task, "retry"),
      onRequeue: () => openRestart(task, "requeue"),
      onViewSession: () => openSession(task),
      onMerge: () => openMerge(task),
      onUnqueueMerge: () => void act(() => workTaskMergeUnqueue(task.id)),
      onComplete: () => openComplete(task),
      onArchive: () =>
        void act(() => workTaskArchive(task.id, task.archived_at == null)),
      onEdit: () => {
        setEditorTask(task)
        setEditorOpen(true)
      },
      onSchedule: () => openSchedule(task),
    }),
    [
      act,
      openCancel,
      openAssignSession,
      openComplete,
      openMerge,
      openRestart,
      openSchedule,
      openSession,
      openNewSession,
    ]
  )

  const handleTaskDragStart = useCallback((event: DragStartEvent) => {
    const taskId = taskIdFromDragId(event.active.id)
    const task = tasksRef.current.find((row) => row.id === taskId)
    if (!task || !isBoardDraggable(task)) return
    draggedRef.current = true
    setDrag({ task })
  }, [])

  const handleTaskDragOver = useCallback((event: DragOverEvent) => {
    setDropTarget(columnFromDropId(event.over?.id))
  }, [])

  const clearDrag = useCallback((afterDeliberateDrop: boolean) => {
    setDrag(null)
    setDropTarget(null)
    if (!afterDeliberateDrop) {
      draggedRef.current = false
      return
    }
    requestAnimationFrame(() => {
      draggedRef.current = false
    })
  }, [])

  // Moving a neutral/manual card only changes its board status. Starting or
  // waking an Agent remains an explicit assign/claim action.
  const handleTaskDragEnd = useCallback(
    (event: DragEndEvent) => {
      const taskId = taskIdFromDragId(event.active.id)
      const target = columnFromDropId(event.over?.id)
      const live = tasksRef.current.find((row) => row.id === taskId)
      clearDrag(true)
      if (
        target != null &&
        live != null &&
        target !== columnForStatus(live.task_status) &&
        (live.execution_mode == null || live.execution_mode === "manual")
      ) {
        void act(() =>
          workTaskSetManualStatus(
            live.id,
            live.task_status,
            statusForColumn(target)
          )
        )
      }
    },
    [act, clearDrag]
  )

  // Archives whatever the Done column currently shows unarchived — respects
  // the visibility toggles by construction (it reads the grouped column).
  const archiveAllDone = useCallback(() => {
    const targets = columns.done.filter((task) => task.archived_at == null)
    if (targets.length === 0) return
    void act(() =>
      Promise.all(targets.map((task) => workTaskArchive(task.id, true)))
    )
  }, [act, columns.done])

  // Attention is a focused triage view, not another status filter layered on
  // top of seven empty columns. Its two relevant columns stay visible even if
  // the user's ordinary board display settings hide either one.
  const visibleColumnIds: BoardColumnId[] =
    scope === "attention"
      ? ["review", "blocked"]
      : BOARD_COLUMN_IDS.filter(
          (column) => !boardFilter.hiddenColumns.includes(column)
        )
  const isList = viewMode === "list"
  const hasAnyTask = tasks.length > 0
  // Nothing has arrived yet: draw the page's own shape instead of flashing the
  // "no tasks yet" empty state at someone who has plenty.
  const showSkeleton = loading && !hasAnyTask

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={pointerWithin}
      onDragStart={handleTaskDragStart}
      onDragOver={handleTaskDragOver}
      onDragCancel={() => clearDrag(false)}
      onDragEnd={handleTaskDragEnd}
    >
      <div className="flex h-full min-h-0 flex-col">
        {/* Toolbar (the page title renders in the chrome strip above the page,
          which owns the divider — the toolbar itself is borderless).
          pt-4 / px-4, not py-2: the pills clear the title bar by the same 1rem
          they clear the window's left edge, and pb-2 plus the board's own pt-2
          makes the gap underneath 1rem too — the row sits on one inset.

          Withheld until the first task exists: on an empty board every control
          here filters or starts nothing, and the only action that does
          anything — "new task" — is already the empty state's own button. */}
        {hasAnyTask && (
          <div className="flex shrink-0 flex-wrap items-center gap-2 px-4 pb-2 pt-4">
            {/* Searchable, and each row shows `alias [ name ]` over the folder's
              path — the same list the new-conversation composer opens. Leads
              with a folder glyph like the Automations filter pill: "全部文件夹"
              alone doesn't say WHICH axis the pill filters. */}
            <FolderSelect
              folders={projectFolders}
              value={folderFilter}
              onChange={setFolderFilter}
              allLabel={t("allFolders")}
              onSelectAll={() => setFolderFilter(null)}
            />

            <div
              className="flex items-center gap-0.5"
              role="tablist"
              aria-label={t("scopeFilter")}
            >
              {TASKS_SCOPES.map((item) => (
                <Button
                  key={item}
                  type="button"
                  size="sm"
                  variant="ghost"
                  role="tab"
                  aria-selected={scope === item}
                  className={cn(
                    "h-8 gap-1.5 rounded-lg px-3 text-[0.8125rem] font-medium",
                    scope === item
                      ? "bg-muted text-foreground hover:bg-muted"
                      : "text-muted-foreground hover:text-foreground"
                  )}
                  onClick={() => setScope(item)}
                >
                  {t(SCOPE_LABEL_KEYS[item])}
                  {item === "attention" && attentionCount > 0 ? (
                    <span className="rounded-full bg-amber-500/12 px-1.5 py-0.5 text-[0.625rem] font-semibold leading-none text-amber-700 tabular-nums dark:text-amber-300">
                      {attentionCount}
                    </span>
                  ) : null}
                </Button>
              ))}
            </div>

            {/* Same pill treatment as the folder select so the left cluster reads
              as one family of controls (the settings entry lives in the chrome
              strip next to the page title). */}
            <Popover>
              <PopoverTrigger asChild>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  className="h-8 gap-1.5 rounded-full bg-muted/70 px-3 text-[0.8125rem] font-medium ws-msg-chip hover:bg-muted"
                >
                  <SlidersHorizontal
                    className="size-3.5 text-muted-foreground"
                    aria-hidden="true"
                  />
                  {t("display")}
                </Button>
              </PopoverTrigger>
              <PopoverContent
                align="start"
                className="w-64 gap-0.5 rounded-xl p-1.5"
              >
                <p className="px-2 pb-1 pt-1 text-[0.6875rem] font-medium text-muted-foreground">
                  {t("groupBy")}
                </p>
                <RadioGroup
                  value={grouping}
                  onValueChange={(value) =>
                    setGrouping(value as TasksBoardGrouping)
                  }
                  className="gap-0.5"
                >
                  {TASKS_BOARD_GROUPINGS.map((item) => (
                    <label
                      key={item}
                      className="flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-xs hover:bg-accent/50"
                    >
                      <RadioGroupItem value={item} className="size-3.5" />
                      {t(GROUPING_LABEL_KEYS[item])}
                    </label>
                  ))}
                </RadioGroup>
                <div className="my-1 h-px bg-border" />
                <p className="px-2 pb-1 pt-1 text-[0.6875rem] font-medium text-muted-foreground">
                  {t("sortBy")}
                </p>
                <RadioGroup
                  value={sort}
                  onValueChange={(value) => setSort(value as TasksSort)}
                  className="gap-0.5"
                >
                  {TASKS_SORTS.map((item) => (
                    <label
                      key={item}
                      className="flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-xs hover:bg-accent/50"
                    >
                      <RadioGroupItem value={item} className="size-3.5" />
                      <ArrowDownWideNarrow
                        className="size-3.5 text-muted-foreground"
                        aria-hidden="true"
                      />
                      {t(SORT_LABEL_KEYS[item])}
                    </label>
                  ))}
                </RadioGroup>
                <div className="my-1 h-px bg-border" />
                <p className="px-2 pb-1 pt-1 text-[0.6875rem] font-medium text-muted-foreground">
                  {t("showColumns")}
                </p>
                {BOARD_COLUMN_IDS.map((column) => {
                  const forcedByAttention =
                    scope === "attention" &&
                    (column === "review" || column === "blocked")
                  return (
                    <label
                      key={column}
                      className={cn(
                        "flex items-center gap-2 rounded-lg px-2 py-1.5 text-xs",
                        forcedByAttention
                          ? "cursor-default text-muted-foreground"
                          : "cursor-pointer hover:bg-accent/50"
                      )}
                    >
                      <Checkbox
                        checked={
                          forcedByAttention ||
                          !boardFilter.hiddenColumns.includes(column)
                        }
                        disabled={forcedByAttention}
                        onCheckedChange={(checked) =>
                          setBoardFilter((filter) => {
                            if (checked === true) {
                              return {
                                ...filter,
                                hiddenColumns: filter.hiddenColumns.filter(
                                  (item) => item !== column
                                ),
                              }
                            }
                            if (
                              filter.hiddenColumns.length >=
                              BOARD_COLUMN_IDS.length - 1
                            )
                              return filter
                            return {
                              ...filter,
                              hiddenColumns: [...filter.hiddenColumns, column],
                            }
                          })
                        }
                      />
                      <TaskColumnIcon column={column} />
                      {t(COLUMN_LABEL_KEYS[column])}
                    </label>
                  )
                })}
                <div className="my-1 h-px bg-border" />
                <label className="flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-xs hover:bg-accent/50">
                  <Checkbox
                    checked={boardFilter.showArchived}
                    onCheckedChange={(v) =>
                      setBoardFilter((f) => ({
                        ...f,
                        showArchived: v === true,
                      }))
                    }
                  />
                  {t("showArchived")}
                </label>
              </PopoverContent>
            </Popover>

            {ownerOptions.length > 0 ? (
              <Select
                value={
                  ownerFilter == null ? "__all_owners__" : String(ownerFilter)
                }
                onValueChange={(value) =>
                  setOwnerFilter(
                    value === "__all_owners__" ? null : Number(value)
                  )
                }
              >
                <SelectTrigger
                  size="sm"
                  aria-label={t("ownerFilter")}
                  className="h-8 w-auto max-w-48 gap-1.5 rounded-full border-transparent bg-muted/70 px-3 text-[0.8125rem] font-medium shadow-none ws-msg-chip hover:bg-muted"
                >
                  <UserRound
                    className="size-3.5 shrink-0 text-muted-foreground"
                    aria-hidden="true"
                  />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__all_owners__">
                    {t("ownerAll")}
                  </SelectItem>
                  {ownerOptions.map((option) => (
                    <SelectItem key={option.id} value={String(option.id)}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : null}

            {/* Status filter — list-only: the board already sorts by status into
              its seven columns, so there is nothing there for it to narrow.
              One choice out of six (including All), so a select rather than a
              checkbox menu; it offers the same seven columns as the board. */}
            {isList ? (
              <Select
                value={statusFilter ?? ALL_STATUSES}
                onValueChange={(v) =>
                  setStatusFilter(
                    v === ALL_STATUSES ? null : (v as BoardColumnId)
                  )
                }
              >
                {/* "状态: 等你处理" — the axis AND the value, like the composer's
                  mode selector: a bare aria-label would replace the value with
                  the axis name, and no label at all leaves the axis unsaid. */}
                <SelectTrigger
                  size="sm"
                  aria-label={`${t("statusFilter")}: ${
                    statusFilter == null
                      ? t("statusFilterAll")
                      : t(COLUMN_LABEL_KEYS[statusFilter])
                  }`}
                  className="h-8 w-auto min-w-0 gap-1.5 rounded-full border-transparent bg-muted/70 px-3 text-[0.8125rem] font-medium shadow-none ws-msg-chip hover:bg-muted"
                >
                  <Tag
                    className="size-3.5 text-muted-foreground"
                    aria-hidden="true"
                  />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={ALL_STATUSES}>
                    {t("statusFilterAll")}
                  </SelectItem>
                  {BOARD_COLUMN_IDS.map((col) => (
                    <SelectItem key={col} value={col}>
                      <span className="flex items-center gap-2">
                        <TaskColumnIcon column={col} />
                        {t(COLUMN_LABEL_KEYS[col])}
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : null}

            <div className="flex-1" />

            <Button
              type="button"
              size="sm"
              className="h-8 gap-1 rounded-full px-3.5 text-[0.8125rem]"
              onClick={openNewTask}
            >
              <Plus className="size-4" aria-hidden="true" />
              {t("new")}
            </Button>
          </div>
        )}

        {/* Board / list */}
        {showSkeleton ? (
          <TasksSkeleton mode={viewMode} />
        ) : !hasAnyTask ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center">
            <ListTodo
              className="size-10 text-muted-foreground/40"
              aria-hidden="true"
            />
            <div className="flex flex-col gap-1">
              <p className="text-sm font-medium">{t("empty")}</p>
              <p className="max-w-sm text-xs text-muted-foreground">
                {t("emptyHint")}
              </p>
            </div>
            <Button
              type="button"
              size="sm"
              className="gap-1.5"
              onClick={openNewTask}
            >
              <Plus className="size-3.5" aria-hidden="true" />
              {t("new")}
            </Button>
          </div>
        ) : isList ? (
          <TaskList
            tasks={listTasks}
            folderNames={folderNames}
            conversationNames={conversationNames}
            now={now}
            mergeQueueRanks={queueRanks}
            filtered={statusFilter != null}
            onClearFilter={() => setStatusFilter(null)}
            onOpen={setDetailTaskId}
            handlersFor={handlersFor}
            segments={listSegments}
            showGroupHeaders={showGroupHeaders}
            ungroupedLabel={t("groupUngrouped")}
            activityFor={activityFor}
          />
        ) : (
          <div className="min-h-0 flex-1 overflow-x-auto">
            {/* pt-2, the same as the list view's card: the two views share this
              rail, so toggling between them must not nudge the content up or
              down. With the toolbar's pb-2 that is 16px under the pills — the
              pt-4 they keep above themselves. */}
            <div
              className="grid h-full gap-4 px-4 pb-4 pt-2"
              style={{
                gridTemplateColumns: `repeat(${visibleColumnIds.length}, minmax(13rem, 1fr))`,
                minWidth: `${Math.max(1, visibleColumnIds.length) * 14}rem`,
              }}
            >
              {visibleColumnIds.map((col) => {
                const colTasks = columns[col]
                const segments = columnSegments[col]
                const cardFor = (task: WorkTask) => (
                  <TaskCard
                    key={task.id}
                    task={task}
                    folderName={folderNames.get(task.folder_id) ?? null}
                    ownerLabel={
                      task.conversation_id == null
                        ? null
                        : (conversationNames.get(task.conversation_id) ??
                          `Session #${task.conversation_id}`)
                    }
                    now={now}
                    mergeQueueRank={queueRanks.get(task.id)}
                    activity={activityFor(task)}
                    onOpen={() => {
                      // Swallow the click that closes a drag.
                      if (draggedRef.current) return
                      setDetailTaskId(task.id)
                    }}
                    {...handlersFor(task)}
                  />
                )
                return (
                  <TaskColumnDropZone key={col} column={col}>
                    <div className="flex h-6 shrink-0 items-center gap-2 px-0.5">
                      <TaskColumnIcon column={col} className="size-3.5" />
                      <h2 className="text-xs font-semibold">
                        {t(COLUMN_LABEL_KEYS[col])}
                      </h2>
                      {/* The count is a pill in the toolbar's muted-pill
                        language, so the two header rows read as one block. */}
                      <span className="rounded-full bg-muted/70 px-1.5 py-0.5 text-[0.625rem] font-medium leading-none text-muted-foreground tabular-nums">
                        {colTasks.length}
                      </span>
                      <div className="flex-1" />
                      {col === "done" &&
                      columns.done.some((task) => task.archived_at == null) ? (
                        <Button
                          type="button"
                          size="xs"
                          variant="ghost"
                          className="px-1.5 font-normal text-muted-foreground hover:text-foreground"
                          onClick={archiveAllDone}
                        >
                          {t("archiveAllDone")}
                        </Button>
                      ) : null}
                      <Button
                        type="button"
                        size="icon-xs"
                        variant="ghost"
                        aria-label={`${t("new")} · ${t(COLUMN_LABEL_KEYS[col])}`}
                        onClick={() => openNewTaskInColumn(col)}
                      >
                        <Plus className="size-3.5" aria-hidden="true" />
                      </Button>
                    </div>
                    {colTasks.length === 0 ? (
                      <div
                        className={cn(
                          // border-border is near-invisible on the plain canvas —
                          // dash with a foreground-derived tone instead.
                          "flex flex-1 flex-col items-center justify-center gap-1.5 rounded-xl border border-dashed border-muted-foreground/30 p-4 text-center",
                          // Drop target while a pending card is dragged: quiet
                          // once the drag starts, loud once the pointer is
                          // actually inside. Otherwise tint like the cards when a
                          // workspace background image is on (ws-msg-card is
                          // inert without one) so the cell stays legible over a
                          // photo.
                          drag
                            ? dropTarget === col
                              ? "border-primary bg-primary/10"
                              : "border-primary/40"
                            : "ws-msg-card"
                        )}
                      >
                        <p className="text-xs text-muted-foreground">
                          {t(EMPTY_LABEL_KEYS[col])}
                        </p>
                      </div>
                    ) : (
                      <ScrollArea
                        className={cn(
                          "min-h-0 flex-1 rounded-xl transition-colors",
                          drag &&
                            (dropTarget === col
                              ? "bg-primary/5 ring-2 ring-primary"
                              : "ring-1 ring-primary/25")
                        )}
                      >
                        {showGroupHeaders ? (
                          <div className={CARD_LIST_CLASS}>
                            {segments.map((seg) => (
                              <div
                                key={seg.key}
                                className="flex flex-col gap-2"
                              >
                                <TaskGroupHeader
                                  label={
                                    seg.ungrouped || !seg.label
                                      ? t("groupUngrouped")
                                      : seg.label
                                  }
                                  count={seg.tasks.length}
                                />
                                <DraggableTaskList
                                  tasks={seg.tasks}
                                  dragId={drag?.task.id}
                                  renderCard={cardFor}
                                  className="flex flex-col gap-4"
                                />
                              </div>
                            ))}
                          </div>
                        ) : (
                          <DraggableTaskList
                            tasks={colTasks}
                            dragId={drag?.task.id}
                            renderCard={cardFor}
                            className={CARD_LIST_CLASS}
                          />
                        )}
                      </ScrollArea>
                    )}
                  </TaskColumnDropZone>
                )
              })}
            </div>
          </div>
        )}

        <TaskEditorDialog
          open={editorOpen}
          onOpenChange={(o) => {
            setEditorOpen(o)
            if (!o) setEditorPrefill(null)
          }}
          task={editorTask}
          defaultInitialStatus={editorInitialStatus}
          defaultFolderId={editorPrefill?.folderId ?? folderFilter}
          prefillText={editorPrefill?.text ?? null}
          onSubmit={submitEditor}
        />
        <TaskDetailSheet
          open={detailTaskId != null}
          onOpenChange={(o) => {
            if (!o) setDetailTaskId(null)
          }}
          task={detailTask}
          folderName={
            detailTask ? (folderNames.get(detailTask.folder_id) ?? null) : null
          }
          onViewSession={openSession}
          onMerge={openMerge}
          onComplete={openComplete}
          onCancel={openCancel}
          onEdit={(task) => {
            setEditorTask(task)
            setEditorOpen(true)
          }}
          onSchedule={openSchedule}
          onAssignSession={openAssignSession}
          onStartAgent={openNewSession}
        />
        {/* The queue state comes from the live row (a merge that starts while
          the dialog is open turns "merge" into "queue"), the form from the
          captured one — see `mergeLiveTask`. */}
        <TaskMergeDialog
          open={mergeOpen}
          onOpenChange={setMergeOpen}
          task={mergeTask}
          folderMerging={
            mergeTask != null && isFolderMerging(tasks, mergeTask.folder_id)
          }
          alreadyQueued={mergeLiveTask != null && isMergeQueued(mergeLiveTask)}
        />
        <TaskCompleteDialog
          open={completeOpen}
          onOpenChange={setCompleteOpen}
          task={completeTask}
        />
        {/* Rendered after the sheet, like the transcript viewer: both portal to
          body and the later mount stacks above, so a cancel / restart asked
          for from inside the drawer lands on top of it. */}
        <TaskCancelDialog
          open={cancelOpen}
          onOpenChange={setCancelOpen}
          task={cancelTask}
        />
        <TaskRestartDialog
          open={restartOpen}
          onOpenChange={setRestartOpen}
          task={restartTask}
          kind={restartKind}
        />
        <TaskScheduleDialog
          open={scheduleOpen && scheduleTask != null}
          onOpenChange={setScheduleOpen}
          task={scheduleTask}
        />
        <TaskAssignSessionDialog
          open={assignOpen && assignTask != null}
          onOpenChange={setAssignOpen}
          task={assignTask}
          sessions={assignableSessions}
          onCreateSession={() => {
            if (!assignTask) return
            setAssignOpen(false)
            setAssignTaskId(null)
            openNewSession(assignTask)
          }}
          onSubmit={async (conversationId) => {
            if (!assignTask) return
            await workTaskAssignSession(assignTask.id, conversationId)
            setAssignOpen(false)
            setAssignTaskId(null)
            void refetch()
          }}
        />
        <TaskSessionLaunchDialog
          open={launchOpen && launchTask != null}
          onOpenChange={(open) => {
            setLaunchOpen(open)
            if (!open) setLaunchTaskId(null)
          }}
          task={launchTask}
          folderPath={
            launchTask
              ? (allFolders.find((folder) => folder.id === launchTask.folder_id)
                  ?.path ?? null)
              : null
          }
          onSubmit={async (config) => {
            if (!launchTask) return
            const folderPath = allFolders.find(
              (folder) => folder.id === launchTask.folder_id
            )?.path
            if (!folderPath) {
              throw new Error("Task project folder is no longer available")
            }
            await createTaskSessionAndAssign({
              task: launchTask,
              folderPath,
              config,
            })
            setLaunchOpen(false)
            setLaunchTaskId(null)
            void refetch()
          }}
        />
        {/* Rendered after the sheet so it stacks above it when opened from
          within (both portal to body; later mount wins). */}
        <TaskTranscriptDialog
          open={sessionOpen && sessionTask != null}
          onOpenChange={setSessionOpen}
          task={sessionTask}
        />
        <TaskSettingsDialog
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
          folderId={folderFilter}
        />

        <DragOverlay dropAnimation={null}>
          {drag ? (
            <div
              className={cn(
                "pointer-events-none flex w-52 -rotate-1 flex-col gap-2 rounded-xl border bg-card p-3 shadow-lg",
                dropTarget != null
                  ? "border-primary ring-2 ring-primary/25"
                  : "border-foreground/15"
              )}
              aria-hidden="true"
            >
              <div className="flex items-start justify-between gap-2">
                <span className="min-w-0 break-words text-[0.8125rem] font-medium leading-snug">
                  {drag.task.title}
                </span>
                <StatusChip task={drag.task} />
              </div>
              {dropTarget ? (
                <span className="inline-flex items-center gap-1 text-[0.6875rem] font-medium text-primary">
                  {t(COLUMN_LABEL_KEYS[dropTarget])}
                </span>
              ) : null}
            </div>
          ) : null}
        </DragOverlay>
      </div>
    </DndContext>
  )
}

function TaskColumnDropZone({
  column,
  children,
}: {
  column: BoardColumnId
  children: React.ReactNode
}) {
  const { setNodeRef } = useDroppable({ id: columnDropId(column) })
  return (
    <div ref={setNodeRef} className="flex min-h-0 flex-col gap-2">
      {children}
    </div>
  )
}

function DraggableTaskList({
  tasks,
  dragId,
  renderCard,
  className,
}: {
  tasks: WorkTask[]
  dragId: number | undefined
  renderCard: (task: WorkTask) => React.ReactNode
  className?: string
}) {
  return (
    <div className={className}>
      {tasks.map((task) => (
        <DraggableTaskCard
          key={task.id}
          task={task}
          active={dragId === task.id}
        >
          {renderCard(task)}
        </DraggableTaskCard>
      ))}
    </div>
  )
}

function DraggableTaskCard({
  task,
  active,
  children,
}: {
  task: WorkTask
  active: boolean
  children: React.ReactNode
}) {
  const draggable = isBoardDraggable(task)
  const { listeners, setNodeRef } = useDraggable({
    id: taskDragId(task.id),
    disabled: !draggable,
  })
  return (
    <div
      ref={setNodeRef}
      {...(draggable ? listeners : {})}
      className={cn(
        draggable && "touch-none cursor-grab active:cursor-grabbing",
        active && "opacity-40"
      )}
    >
      {children}
    </div>
  )
}
