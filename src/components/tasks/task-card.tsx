"use client"

import { useTranslations } from "next-intl"
import {
  Bot,
  CalendarClock,
  Check,
  CircleAlert,
  CircleCheck,
  CircleX,
  FolderX,
  GitMerge,
  ListTodo,
  Loader2,
} from "lucide-react"
import { AgentIcon } from "@/components/agent-icon"
import { Button } from "@/components/ui/button"
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import { formatRelative } from "@/components/conversations/sidebar-conversation-grouping"
import { formatScheduleFull, formatScheduleShort } from "@/lib/task-schedule"
import { cn } from "@/lib/utils"
import { isMergeQueued, worktreeWasRemoved } from "./task-acceptance"
import { buildTaskActions, type TaskActionItem } from "./task-actions"
import type { TaskActionHandlers } from "./task-actions"
import type { TaskActivityDot } from "./task-activity"
import type { WorkTask } from "@/lib/types"

type StatusLabelKey =
  | "statusTodo"
  | "statusQueued"
  | "statusPreparing"
  | "statusRunning"
  | "statusAwaitingInput"
  | "statusReview"
  | "statusMerging"
  | "statusDone"
  | "statusFailed"
  | "statusCanceled"
  | "statusBlocked"

export function statusLabelKey(status: WorkTask["status"]): StatusLabelKey {
  switch (status) {
    case "todo":
      return "statusTodo"
    case "queued":
      return "statusQueued"
    case "preparing":
      return "statusPreparing"
    case "running":
      return "statusRunning"
    case "awaiting_input":
      return "statusAwaitingInput"
    case "review":
      return "statusReview"
    case "merging":
      return "statusMerging"
    case "done":
      return "statusDone"
    case "failed":
      return "statusFailed"
    case "canceled":
      return "statusCanceled"
  }
}

/**
 * Per-status presentation, so the board reads by shape rather than nine
 * same-looking chips: live statuses are primary-colored spinner text,
 * `awaiting_input` an outlined amber pill with a light pulse, `review` a
 * muted pill (neutral among the attention column), `merging` a muted
 * spinner, `done` a bare green check, `failed` a tinted red pill, the
 * rest a muted pill.
 *
 * The label truncates inside whatever width it is given (with the full text on
 * `title`): the list view puts the chip in a fixed status column, and locales
 * whose "awaiting input" runs to twenty characters must not blow that column
 * out. In the card, where the chip sizes to its content, this never engages.
 */
export function StatusChip({
  task,
  className,
}: {
  task: WorkTask
  className?: string
}) {
  const t = useTranslations("Tasks")
  const visualStatus = taskVisualStatus(task)
  // An interrupted failure (restart) reads differently from an agent failure.
  const label =
    task.execution_mode !== "engine" && task.task_status === "backlog"
      ? t("statusBacklog")
      : visualStatus === "failed" && task.failure_reason === "interrupted"
        ? t("statusInterrupted")
        : task.execution_mode !== "engine" && task.task_status === "blocked"
          ? t("statusBlocked")
          : t(statusLabelKey(visualStatus))

  let tone: string
  let icon: React.ReactNode = null
  switch (visualStatus) {
    case "queued":
    case "preparing":
    case "running":
      tone =
        task.execution_mode === "engine"
          ? "gap-1 text-[0.6875rem] text-primary"
          : "text-[0.6875rem] text-primary"
      icon =
        task.execution_mode === "engine" ? (
          <Loader2
            className="size-3 shrink-0 animate-spin"
            aria-hidden="true"
          />
        ) : null
      break
    case "merging":
      tone = "gap-1 text-[0.6875rem] text-muted-foreground"
      icon = (
        <Loader2 className="size-3 shrink-0 animate-spin" aria-hidden="true" />
      )
      break
    case "awaiting_input":
      // Amber tokens match the attention column marker (`bg-amber-500` /
      // `text-amber-600` / `dark:text-amber-400` in tasks-page.tsx).
      tone =
        "gap-1 rounded-full border border-amber-500/45 bg-amber-500/5 px-2 py-1 text-[0.625rem] text-amber-600 dark:border-amber-400/40 dark:text-amber-400"
      icon =
        task.execution_mode === "engine" ? (
          <span
            className="size-1.5 shrink-0 animate-pulse rounded-full bg-amber-500"
            aria-hidden="true"
          />
        ) : null
      break
    case "done":
      tone =
        "gap-1 rounded-full bg-emerald-500/10 px-2 py-1 text-[0.625rem] text-emerald-600 dark:text-emerald-400"
      icon = (
        <Check
          className="size-2.5 shrink-0"
          strokeWidth={3}
          aria-hidden="true"
        />
      )
      break
    case "failed":
      tone =
        "rounded-full bg-destructive/10 px-2 py-1 text-[0.625rem] text-destructive"
      break
    default:
      tone =
        "rounded-full bg-muted px-2 py-1 text-[0.625rem] text-muted-foreground"
  }

  return (
    <span
      className={cn(
        "inline-flex min-w-0 shrink-0 items-center font-medium leading-none",
        tone,
        className
      )}
      title={label}
    >
      {icon}
      <span className="truncate">{label}</span>
    </span>
  )
}

function taskVisualStatus(task: WorkTask): WorkTask["status"] {
  // Only the WorkTask engine owns the legacy execution status axis. Manual,
  // unassigned, and persistent-Session cards all render their business state.
  if (task.execution_mode === "engine") {
    return task.status
  }
  switch (task.task_status) {
    case "backlog":
      return "todo"
    case "todo":
      return "todo"
    case "in_progress":
      return "running"
    case "blocked":
      return "awaiting_input"
    case "review":
      return "review"
    case "done":
      return "done"
    case "canceled":
      return "canceled"
  }
}

export function ExecutionModeChip({ task }: { task: WorkTask }) {
  const t = useTranslations("Tasks")
  const label =
    task.execution_mode === null
      ? t("executionUnassigned")
      : task.execution_mode === "manual"
        ? t("executionManual")
        : task.execution_mode === "session"
          ? t("executionSession")
          : t("executionAgent")
  return (
    <span className="rounded-full bg-muted px-1.5 py-0.5 text-[0.625rem] text-muted-foreground">
      {label}
    </span>
  )
}

/**
 * The list row's leading accent bar — the board's column markers turned on
 * their side, so the two views speak one colour language.
 *
 * It refines the mapping where a board column lumps four outcomes together:
 * `failed` reads red, `awaiting_input` keeps the attention column's amber
 * (with a light pulse), `review` is a neutral bar, `merging` is muted, and
 * `canceled` stays neutral instead of inheriting Done's green (a green bar
 * on a row that says "已取消" is a lie the board gets away with only because
 * its column heading says "Done" once, at the top).
 */
export function statusAccent(task: WorkTask): string {
  if (task.archived_at != null) return "bg-muted-foreground/20"
  switch (taskVisualStatus(task)) {
    case "todo":
    case "queued":
      return "bg-muted-foreground/35"
    case "preparing":
    case "running":
      return "bg-primary"
    case "awaiting_input":
      return task.execution_mode === "engine"
        ? "animate-pulse bg-amber-500"
        : "bg-amber-500"
    case "review":
      return "bg-muted-foreground/35"
    case "merging":
      return "bg-muted-foreground/25"
    case "failed":
      return "bg-destructive"
    case "done":
      return "bg-emerald-500"
    case "canceled":
      return "bg-muted-foreground/25"
  }
}

/**
 * Attention-column surface weight on the card and list row: failed is a red
 * wash, awaiting_input reuses the board's amber tokens, review stays
 * unstyled (neutral), merging is faded. Empty for every other status.
 */
export function attentionSurfaceClass(task: WorkTask): string {
  if (task.archived_at != null) return ""
  switch (taskVisualStatus(task)) {
    case "failed":
      return "border-destructive/50 bg-destructive/5"
    case "awaiting_input":
      return "border-amber-500/45 bg-amber-500/5"
    case "merging":
      return "opacity-70"
    default:
      return ""
  }
}

interface TaskCardProps extends TaskActionHandlers {
  task: WorkTask
  folderName: string | null
  /** Shared render-tick timestamp for relative times (refreshed by the page). */
  now: number
  /** Place in line when this task is waiting to merge (see `mergeQueueRanks`);
   *  the page computes it, because a card cannot see its siblings. */
  mergeQueueRank?: number
  /** Live-session activity: running+Prompting pulses, running otherwise
   *  is still. Absent / null draws nothing (not running, or no session). */
  activity?: TaskActivityDot | null
  onOpen: () => void
}

/**
 * "Accepted, waiting for the project's merge slot." Merges into one base branch
 * run one at a time, so a second acceptance takes a place in line instead of
 * failing — and a row that says nothing about that reads as if the click was
 * lost. Amber like the review states it sits among, with the rank spelled out
 * whenever the page knows it (`第 2 位` is the difference between "queued" and
 * "queued behind one other").
 */
export function MergeQueuedChip({
  task,
  rank,
}: {
  task: WorkTask
  rank?: number
}) {
  const t = useTranslations("Tasks")
  if (!isMergeQueued(task)) return null
  const label =
    rank != null && rank > 1
      ? t("badgeMergeQueuedRank", { rank })
      : t("badgeMergeQueued")
  return (
    <span
      className="inline-flex min-w-0 max-w-full items-center gap-1 rounded-full bg-amber-500/10 px-1.5 py-0.5 text-[0.625rem] font-medium leading-none text-amber-600 dark:text-amber-400"
      title={t("badgeMergeQueuedHint")}
    >
      <GitMerge className="size-2.5 shrink-0" aria-hidden="true" />
      <span className="truncate">{label}</span>
    </span>
  )
}

/** The acceptance red/green light for a reviewed card. */
export function PreflightChip({ task }: { task: WorkTask }) {
  const t = useTranslations("Tasks")
  const light = task.preflight
  if (!light || task.status !== "review") return null
  const tone =
    light.status === "passed"
      ? "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
      : light.status === "failed"
        ? "bg-destructive/10 text-destructive"
        : "bg-muted text-muted-foreground"
  return (
    <span
      className={cn(
        // max-w-full: a long command wraps the chip to its own row in the
        // flex-wrap meta line and then truncates instead of overflowing.
        "inline-flex min-w-0 max-w-full items-center gap-1 rounded-full px-1.5 py-0.5 text-[0.625rem] font-medium leading-none",
        tone
      )}
      title={
        light.status === "passed"
          ? t("preflightPassed", { name: light.command })
          : light.status === "failed"
            ? t("preflightFailed", { name: light.command })
            : t("preflightRunning", { name: light.command })
      }
    >
      {light.status === "running" ? (
        <Loader2 className="size-2.5 animate-spin" aria-hidden="true" />
      ) : light.status === "passed" ? (
        <CircleCheck className="size-2.5" aria-hidden="true" />
      ) : (
        <CircleX className="size-2.5" aria-hidden="true" />
      )}
      <span className="truncate">{light.command}</span>
    </span>
  )
}

/**
 * "The worktree this task ran in has been deleted" — shown in EVERY status
 * (a reviewed task explains its Complete button, a canceled one warns that a
 * requeue starts over, a done one explains why there is no diff). The one
 * exception is built into the predicate: a just-created task that never
 * initialized has nothing removed to report.
 */
export function WorktreeRemovedChip({ task }: { task: WorkTask }) {
  const t = useTranslations("Tasks")
  if (!worktreeWasRemoved(task)) return null
  return (
    <span
      className="inline-flex min-w-0 max-w-full items-center gap-1 rounded-full bg-amber-500/10 px-1.5 py-0.5 text-[0.625rem] font-medium leading-none text-amber-600 dark:text-amber-400"
      title={t("badgeWorktreeRemoved")}
    >
      <FolderX className="size-2.5 shrink-0" aria-hidden="true" />
      <span className="truncate">{t("badgeWorktreeRemoved")}</span>
    </span>
  )
}

/**
 * The mark of the agent on this task, drawn beside the title in BOTH views —
 * which agent is on a task belongs to the task, so the board and the list must
 * answer it identically. The backend resolves it the way the engine does at
 * launch (see `agent_type`), so a task that simply inherits its folder's
 * settings still shows the mark it will actually run under; `config.agent_type`
 * covers a payload that reached the client without that stamp.
 *
 * Sized to the title's line rather than framed like the detail sheet's glyph: a
 * row or card carries one, and at this density a bare mark reads as part of the
 * title instead of as another chip. Left to name itself through the mark's own
 * `<title>`, as every other AgentIcon in the app is — the words for it are in
 * the detail sheet, beside a glyph big enough to deserve them.
 *
 * The box is drawn even when no agent is configured anywhere (the one state the
 * engine refuses to launch): both views align their titles on it, and a
 * placeholder is worth more than a column that shifts row to row.
 */
/** Agent mark with the optional live-session activity dot overlaid the same
 *  way the workbench tree (O34) overlays a Session. The wrap is skipped when
 *  there is no dot, so a card that isn't running keeps today's DOM. */
export function TaskAgentMarkWithActivity({
  task,
  activity,
  className,
}: {
  task: WorkTask
  activity?: TaskActivityDot | null
  className?: string
}) {
  if (!activity) {
    return <TaskAgentMark task={task} className={className} />
  }
  return (
    <span className={cn("relative shrink-0", className)}>
      <TaskAgentMark task={task} />
      <TaskActivityDotMark kind={activity} />
    </span>
  )
}

/** Green pulse (Prompting) or still dot (running, waiting on a tool). */
export function TaskActivityDotMark({ kind }: { kind: TaskActivityDot }) {
  return (
    <span
      data-testid="task-activity-dot"
      data-pulse={kind === "pulse" ? "true" : "false"}
      className={cn(
        "absolute -top-0.5 -left-0.5 size-1.5 rounded-full bg-emerald-500",
        kind === "pulse" && "animate-pulse"
      )}
      aria-hidden="true"
    />
  )
}

export function TaskAgentMark({
  task,
  className,
}: {
  task: WorkTask
  className?: string
}) {
  const agentType = task.agent_type ?? task.config?.agent_type ?? null
  if (!agentType) {
    return (
      <Bot
        className={cn("size-3.5 shrink-0 text-muted-foreground/40", className)}
        aria-hidden="true"
      />
    )
  }
  return (
    <AgentIcon agentType={agentType} className={cn("size-3.5", className)} />
  )
}

/**
 * The planned start of a to-do task. Primary-tinted rather than muted: it is
 * the one thing on a pending card that says something WILL happen, and it is
 * how a card that looks idle explains itself.
 */
export function ScheduleChip({ task }: { task: WorkTask }) {
  const t = useTranslations("Tasks")
  if (task.status !== "todo" || !task.scheduled_at) return null
  return (
    <span
      className="inline-flex min-w-0 max-w-full items-center gap-1 rounded-full bg-primary/10 px-1.5 py-0.5 text-[0.625rem] font-medium leading-none text-primary"
      title={t("scheduleBadge", {
        time: formatScheduleFull(task.scheduled_at),
      })}
    >
      <CalendarClock className="size-2.5 shrink-0" aria-hidden="true" />
      <span className="truncate">{formatScheduleShort(task.scheduled_at)}</span>
    </span>
  )
}

/**
 * One board card. The whole card opens the detail sheet; the footer carries
 * the shared action set (see `buildTaskActions`): one filled primary on the
 * left, round icon buttons for the secondaries on the right.
 */
export function TaskCard({
  task,
  folderName,
  now,
  mergeQueueRank,
  activity,
  onOpen,
  ...handlers
}: TaskCardProps) {
  const t = useTranslations("Tasks")
  const archived = task.archived_at != null
  const live =
    task.status === "running" ||
    task.status === "awaiting_input" ||
    task.status === "merging"

  const stat =
    task.files_changed != null && task.files_changed > 0 ? (
      <span className="inline-flex shrink-0 items-center gap-1 font-mono text-[0.625rem]">
        <span className="text-emerald-600 dark:text-emerald-400">
          +{task.additions ?? 0}
        </span>
        <span className="text-destructive">-{task.deletions ?? 0}</span>
      </span>
    ) : null
  const when = formatRelative(
    task.finished_at ?? task.settled_at ?? task.started_at ?? task.created_at,
    now
  )

  const { primary, secondaries } = buildTaskActions(task, t, handlers)

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          role="button"
          tabIndex={0}
          onClick={onOpen}
          onKeyDown={(e) => {
            // Only the card's own focus opens the sheet: Enter/Space on one of the
            // footer buttons bubbles up here, and preventing the default would
            // swallow that button's activation and open the sheet instead.
            if (e.target !== e.currentTarget) return
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault()
              onOpen()
            }
          }}
          className={cn(
            // ws-msg-card: with a workspace background image on, the card goes
            // translucent like message-stream cards (e.g. the file-edit card).
            // border-foreground/15, not border-border: a card is white-on-white
            // here, and the token border all but vanishes on that canvas (the
            // empty column's dashed outline had to be derived the same way).
            "group/card flex cursor-pointer flex-col rounded-xl border border-foreground/15 bg-card ws-msg-card p-3 text-left",
            // Hover is a border colour change only — no lift, no shadow.
            "transition-colors hover:border-primary",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
            attentionSurfaceClass(task),
            archived && "opacity-60"
          )}
        >
          <div className="flex items-start justify-between gap-2">
            {/* mt-[0.125rem] rides the mark on the FIRST line of a title that
            wraps — items-start would otherwise hang it off the block's top
            edge, half a line above the text it belongs to. */}
            <TaskAgentMarkWithActivity
              task={task}
              activity={activity}
              className="mt-[0.125rem]"
            />
            <span className="min-w-0 flex-1 break-words text-[0.8125rem] font-medium leading-snug">
              {task.title}
            </span>
            <StatusChip task={task} />
          </div>

          <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-1 text-[0.6875rem] text-muted-foreground">
            {folderName ? (
              <span className="max-w-40 truncate">{folderName}</span>
            ) : null}
            {folderName && task.work_branch ? (
              <span className="text-muted-foreground/40">/</span>
            ) : null}
            {task.work_branch ? (
              <span className="truncate font-mono text-[0.625rem]">
                {task.work_branch}
              </span>
            ) : null}
            {(folderName || task.work_branch) && (stat || when) ? (
              <span className="text-muted-foreground/40">·</span>
            ) : null}
            {stat}
            {stat && when ? (
              <span className="text-muted-foreground/40">·</span>
            ) : null}
            {when ? <span className="shrink-0">{when}</span> : null}
            <ExecutionModeChip task={task} />
            <ScheduleChip task={task} />
            <MergeQueuedChip task={task} rank={mergeQueueRank} />
            <PreflightChip task={task} />
            <WorktreeRemovedChip task={task} />
            {task.cleanup_state === "failed" ? (
              <span className="rounded-full bg-amber-500/10 px-1.5 py-0.5 text-[0.625rem] text-amber-600 dark:text-amber-400">
                {t("badgeCleanupFailed")}
              </span>
            ) : null}
            {/* "Kept" claims the worktree is still there — stay silent when its
            directory is actually gone (the removed chip above speaks then). */}
            {task.status === "canceled" &&
            task.worktree_folder_id != null &&
            task.worktree_missing !== true ? (
              <span className="rounded-full bg-muted px-1.5 py-0.5 text-[0.625rem]">
                {t("badgeWorktreeKept")}
              </span>
            ) : null}
          </div>

          {task.last_error &&
          (task.status === "failed" || task.status === "review") ? (
            <div className="mt-2 flex items-center gap-1.5 rounded-lg bg-destructive/10 px-2 py-1.5 text-[0.6875rem] text-destructive">
              <CircleAlert className="size-3.5 shrink-0" aria-hidden="true" />
              <span className="min-w-0 flex-1 truncate">{task.last_error}</span>
              <button
                type="button"
                className="shrink-0 font-medium hover:underline"
                onClick={(e) => {
                  e.stopPropagation()
                  onOpen()
                }}
              >
                {t("errorView")}
              </button>
            </div>
          ) : null}
          {task.status === "review" && task.result_summary ? (
            <p className="mt-1.5 line-clamp-2 text-[0.6875rem] leading-snug text-muted-foreground">
              {task.result_summary}
            </p>
          ) : null}
          {live && task.latest_progress ? (
            <p className="mt-1.5 line-clamp-2 text-[0.6875rem] leading-snug text-muted-foreground italic">
              {task.latest_progress}
            </p>
          ) : null}

          {/* mt-3 / pt-3 = the card's own p-3: the divider clears the content above
          it by the same 12px the buttons clear the card's left, right and
          bottom edges, so the footer sits on one even inset. */}
          {primary || secondaries.length > 0 ? (
            <div className="mt-3 flex items-center gap-1.5 border-t border-border/60 pt-3">
              {primary ? (
                <Button
                  type="button"
                  size="xs"
                  onClick={(e) => {
                    // The card itself opens the detail sheet — keep actions local.
                    e.stopPropagation()
                    primary.onClick()
                  }}
                >
                  <primary.icon className="size-3" aria-hidden="true" />
                  {primary.label}
                </Button>
              ) : null}
              <div className="flex-1" />
              {/* Secondaries are icon-only and round: they are one-per-status at
              most, so a "…" menu just hid them behind an extra click. The
              session viewer always sorts last, anchoring the corner. They
              fade in on hover (opacity only — the row keeps its height, so
              nothing reflows) and on keyboard focus. */}
              {secondaries.map((item) => (
                <CardIconAction key={item.label} item={item} />
              ))}
            </div>
          ) : null}
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent className="min-w-52">
        <ContextMenuItem onSelect={onOpen}>
          <ListTodo className="size-4" aria-hidden="true" />
          {t("detailDescription")}
        </ContextMenuItem>
        {(primary || secondaries.length > 0) && <ContextMenuSeparator />}
        {primary ? (
          <ContextMenuItem onSelect={primary.onClick}>
            <primary.icon className="size-4" aria-hidden="true" />
            {primary.label}
          </ContextMenuItem>
        ) : null}
        {secondaries.map((item) => (
          <ContextMenuItem key={item.label} onSelect={item.onClick}>
            <item.icon className="size-4" aria-hidden="true" />
            {item.label}
          </ContextMenuItem>
        ))}
      </ContextMenuContent>
    </ContextMenu>
  )
}

function CardIconAction({ item }: { item: TaskActionItem }) {
  return (
    <Button
      type="button"
      size="icon-xs"
      variant="outline"
      className="rounded-full opacity-0 transition-opacity focus-visible:opacity-100 group-focus-within/card:opacity-100 group-hover/card:opacity-100"
      title={item.label}
      aria-label={item.label}
      onClick={(e) => {
        // The card itself opens the detail sheet — keep actions local.
        e.stopPropagation()
        item.onClick()
      }}
    >
      <item.icon aria-hidden="true" />
    </Button>
  )
}
