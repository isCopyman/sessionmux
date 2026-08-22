"use client"

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import { useTranslations } from "next-intl"
import { workTaskList } from "@/lib/api"
import { sendSystemNotification } from "@/lib/notification"
import { onTransportReconnect, subscribe } from "@/lib/platform"
import {
  loadTasksViewMode,
  saveTasksViewMode,
  type TasksViewMode,
} from "@/lib/tasks-board-filter-storage"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import type { WorkTask } from "@/lib/types"

const WORK_TASK_CHANGED_EVENT = "task://changed"

/** Business statuses that need the user ("等你处理") — the same axis the
 * board columns use. Engine status only refines the notification wording. */
const ATTENTION_STATUSES = new Set<WorkTask["task_status"]>([
  "blocked",
  "review",
])

interface PreviousTaskState {
  taskStatus: WorkTask["task_status"]
  engineStatus: WorkTask["status"]
}

interface TasksViewContextValue {
  tasks: WorkTask[]
  /** Count of tasks waiting on the user — the sidebar badge. */
  attentionCount: number
  /** True until the first fetch settles (success OR failure). Lets the board
   *  tell "still loading" from "genuinely empty" and show a skeleton instead of
   *  flashing the empty state. Never flips back on later refetches — those
   *  update an already-painted board. */
  loading: boolean
  refetch: () => Promise<void>
  /** Board ⇄ list. Lifted here rather than owned by TasksPage because the
   *  switch renders in the window-chrome strip (TasksPageTitle) — a different
   *  branch of the tree — while the layout it drives renders in the page. */
  viewMode: TasksViewMode
  setViewMode: (mode: TasksViewMode) => void
}

const TasksViewContext = createContext<TasksViewContextValue | null>(null)

/**
 * Data layer for the Tasks feature: the full task list + a realtime
 * subscription, kept always-mounted so the sidebar's attention badge stays
 * live. Single source for both the badge and the Tasks route page (the board
 * filters per folder client-side). Mirrors AutomationsViewProvider: the engine
 * runs headless, so `task://changed` nudges + refetch are the only way an open
 * board learns a task advanced.
 */
export function useTasksView() {
  const ctx = useContext(TasksViewContext)
  if (!ctx) {
    throw new Error("useTasksView must be used within TasksViewProvider")
  }
  return ctx
}

/**
 * Read the always-mounted task projection when this surface happens to live
 * inside the workspace shell. Shared presentation components (notably the
 * Session task banner) are also rendered in isolated tests and previews;
 * those should simply omit the task section instead of inventing a second
 * fetch path or requiring a fake provider.
 */
export function useOptionalTasksView() {
  return useContext(TasksViewContext)
}

export function TasksViewProvider({ children }: { children: ReactNode }) {
  const t = useTranslations("Tasks")
  // Latest-ref so `refetch` stays referentially stable across locale changes
  // (its identity re-subscribes the event channel). Synced in an effect —
  // writing during render trips react-hooks/refs.
  const tRef = useRef(t)
  useEffect(() => {
    tRef.current = t
  }, [t])
  const [tasks, setTasks] = useState<WorkTask[]>([])
  const [loading, setLoading] = useState(true)
  // Restored synchronously from localStorage: the Tasks route mounts only after
  // a client-side route switch (never prerendered), so there is no SSR markup to
  // mismatch — and the page paints in the remembered mode right away.
  const [viewMode, setViewMode] = useState<TasksViewMode>(loadTasksViewMode)
  useEffect(() => {
    saveTasksViewMode(viewMode)
  }, [viewMode])
  const reqRef = useRef(0)
  // Statuses as of the last successful fetch; null until then, so the first
  // load (pure history) never notifies.
  const prevStatusRef = useRef<Map<number, PreviousTaskState> | null>(null)

  const refetch = useCallback(async () => {
    const id = ++reqRef.current
    try {
      const list = await workTaskList(null)
      // Drop stale responses; keep the previous list on transient error rather
      // than blanking the board (same idiom as automations-view-context).
      if (id !== reqRef.current) return
      notifyFlips(prevStatusRef.current, list, tRef.current)
      prevStatusRef.current = new Map(
        list.map((task) => [
          task.id,
          { taskStatus: task.task_status, engineStatus: task.status },
        ])
      )
      setTasks(list)
      setLoading(false)
    } catch {
      // ignore — a later event/refetch recovers. A failed FIRST fetch still
      // ends the loading state (unless a newer one is already in flight): the
      // board falls back to its empty state rather than pulsing forever.
      if (id === reqRef.current) setLoading(false)
    }
  }, [])

  useEffect(() => {
    // Initial fetch + subscribe for backend-pushed nudges. Same
    // subscribe-then-setState idiom as automations-view-context.
    /* eslint-disable react-hooks/set-state-in-effect */
    void refetch()
    let unsub: (() => void) | undefined
    let cancelled = false
    void subscribe(WORK_TASK_CHANGED_EVENT, () => {
      void refetch()
    }).then((u: () => void) => {
      if (cancelled) u()
      else unsub = u
    })
    // Events fired while the WS was disconnected are dropped by the
    // broadcaster; refetch on reconnect so a task that settled during the gap
    // doesn't leave the board stale. No-op on desktop IPC.
    const offReconnect = onTransportReconnect(() => {
      void refetch()
    })
    return () => {
      cancelled = true
      unsub?.()
      offReconnect?.()
    }
    /* eslint-enable react-hooks/set-state-in-effect */
  }, [refetch])

  const attentionCount = useMemo(
    () =>
      tasks.filter(
        (t) => ATTENTION_STATUSES.has(t.task_status) && t.archived_at == null
      ).length,
    [tasks]
  )

  const value = useMemo<TasksViewContextValue>(
    () => ({ tasks, attentionCount, loading, refetch, viewMode, setViewMode }),
    [tasks, attentionCount, loading, refetch, viewMode]
  )

  return (
    <TasksViewContext.Provider value={value}>
      {children}
    </TasksViewContext.Provider>
  )
}

/** Per-task silence for `awaiting_input` flips. `running ⇄ awaiting_input`
 *  can fire on every Question/Permission/PlanApproval; without this the
 *  same card would keep summoning. Review/failed stay unthrottled. */
const AWAITING_INPUT_NOTIFY_COOLDOWN_MS = 5 * 60 * 1000
const lastAwaitingInputNotifyAt = new Map<number, number>()

/** Test-only: wipe the in-memory cooldown so cases don't leak across tests. */
export function resetAwaitingInputNotifyCooldownForTests(): void {
  lastAwaitingInputNotifyAt.clear()
}

type NotifyFlipKey =
  | "notifyReview"
  | "notifyFailed"
  | "notifyAwaitingInput"
  | "notifyBlocked"

/**
 * System notification when a task flips into awaiting_input (blocked on a
 * human answer), review (ready for acceptance), or failed. The engine runs
 * headless, so this fetch-to-fetch diff is the only place that sees the
 * transition; `sendSystemNotification` itself stays silent while the window
 * is visible.
 */
function notifyFlips(
  prev: Map<number, PreviousTaskState> | null,
  list: WorkTask[],
  t: (key: NotifyFlipKey, values: { title: string }) => string
) {
  if (prev == null) return
  for (const task of list) {
    const previous = prev.get(task.id)
    const enteredReview =
      task.task_status === "review" && previous?.taskStatus !== "review"
    const enteredBlocked =
      task.task_status === "blocked" &&
      (previous?.taskStatus !== "blocked" ||
        previous?.engineStatus !== task.status)
    if (!enteredReview && !enteredBlocked) continue
    if (task.archived_at != null) continue
    if (task.status === "awaiting_input") {
      const now = Date.now()
      const last = lastAwaitingInputNotifyAt.get(task.id)
      if (last != null && now - last < AWAITING_INPUT_NOTIFY_COOLDOWN_MS) {
        continue
      }
      lastAwaitingInputNotifyAt.set(task.id, now)
    }
    const folder = useAppWorkspaceStore
      .getState()
      .folders.find((f) => f.id === task.folder_id)
    const folderName = folder ? (folder.alias ?? folder.name) : null
    const title = folderName ? `${folderName} - Codeg` : "Codeg"
    const body = enteredReview
      ? t("notifyReview", { title: task.title })
      : task.status === "awaiting_input"
        ? t("notifyAwaitingInput", { title: task.title })
        : task.status === "failed"
          ? t("notifyFailed", { title: task.title })
          : t("notifyBlocked", { title: task.title })
    void sendSystemNotification(title, body)
  }
}
