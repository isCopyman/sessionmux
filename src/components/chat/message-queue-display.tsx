"use client"

import { useCallback, type PointerEvent } from "react"
import { Reorder, useDragControls } from "motion/react"
import {
  AlertTriangle,
  GripVertical,
  Loader2,
  PauseCircle,
  Pencil,
  Play,
  RotateCcw,
  X,
} from "lucide-react"
import { useTranslations } from "next-intl"
import { cn } from "@/lib/utils"
import type { QueuedMessage } from "@/hooks/use-message-queue"

interface MessageQueueDisplayProps {
  queue: QueuedMessage[]
  onReorder: (items: QueuedMessage[]) => void
  onEdit: (id: string) => void
  onDelete: (id: string) => void
  onRetry: (id: string) => void
  onPauseManual: () => void
  onReleaseOne: (id: string) => void
  onResume: () => void
  pausedReason: string | null
  manualReleaseItemId: string | null
  editingItemId: string | null
}

interface QueueItemProps {
  item: QueuedMessage
  index: number
  isEditing: boolean
  onEdit: (id: string) => void
  onDelete: (id: string) => void
  onRetry: (id: string) => void
  onReleaseOne: (id: string) => void
  manualReview: boolean
  releasePending: boolean
}

// Scheduling-class badge for entries the user did not type themselves.
// Letters and reminders normally queue without a draft (they render in the
// mailbox instead), but any that do surface here must say who they are.
const SOURCE_LABEL_KEY = {
  collaboration: "sourceCollaboration",
  reminder: "sourceReminder",
  automation: "sourceAutomation",
  task: "sourceTask",
  timer: "sourceTimer",
} as const

function QueueItem({
  item,
  index,
  isEditing,
  onEdit,
  onDelete,
  onRetry,
  onReleaseOne,
  manualReview,
  releasePending,
}: QueueItemProps) {
  const t = useTranslations("Folder.chat.messageQueue")
  const dragControls = useDragControls()
  const isClaimed = item.state === "claimed"
  const isPaused = item.state === "paused"
  const isHostOwned = item.taskId != null || item.draft == null
  const canReorder = !isClaimed && item.draft != null

  const startDrag = useCallback(
    (event: PointerEvent<HTMLButtonElement>) => {
      if (!canReorder) return
      event.preventDefault()
      event.stopPropagation()
      dragControls.start(event)
    },
    [canReorder, dragControls]
  )

  return (
    <Reorder.Item
      as="div"
      value={item}
      dragListener={false}
      dragControls={dragControls}
      className={cn(
        "flex items-center gap-1 rounded-md border px-1.5 py-1 text-[10px] leading-none select-none [text-box-trim:both] [text-box-edge:cap_alphabetic]",
        "bg-muted/40 border-border/70",
        isEditing && "border-primary/50 bg-primary/5",
        isPaused && "border-destructive/35 bg-destructive/5"
      )}
    >
      <button
        type="button"
        className={cn(
          "shrink-0 touch-none p-0",
          canReorder
            ? "cursor-grab active:cursor-grabbing"
            : "cursor-default opacity-40"
        )}
        disabled={!canReorder}
        onPointerDown={startDrag}
      >
        <GripVertical className="h-3 w-3 text-muted-foreground/60" />
      </button>
      <span className="shrink-0 font-mono text-[10px] text-muted-foreground/70">
        #{index + 1}
      </span>
      {item.source !== "user" ? (
        <span className="shrink-0 rounded-sm bg-muted-foreground/15 px-1 py-px text-[9px] font-medium text-muted-foreground">
          {t(SOURCE_LABEL_KEY[item.source])}
        </span>
      ) : null}
      <span className="min-w-0 flex-1 truncate text-[10px] text-foreground/80">
        {item.draft?.displayText ??
          (item.source === "user"
            ? t("addToQueue")
            : t(SOURCE_LABEL_KEY[item.source]))}
      </span>
      {isClaimed ? (
        <Loader2
          className="h-2.5 w-2.5 shrink-0 animate-spin text-muted-foreground"
          aria-label={t("dispatching")}
        />
      ) : null}
      {isPaused ? (
        <button
          type="button"
          onClick={() => onRetry(item.id)}
          className="shrink-0 rounded-sm p-0.5 text-destructive hover:bg-destructive/10"
          title={t("retryItem")}
        >
          <RotateCcw className="h-2.5 w-2.5" />
        </button>
      ) : null}
      {manualReview && item.state === "queued" ? (
        <button
          type="button"
          onClick={() => onReleaseOne(item.id)}
          className="shrink-0 rounded-sm p-0.5 text-primary hover:bg-primary/10 disabled:opacity-40"
          title={t("releaseOne")}
          disabled={releasePending}
        >
          <Play className="h-2.5 w-2.5" />
        </button>
      ) : null}
      {!isHostOwned ? (
        <>
          <button
            type="button"
            onClick={() => onEdit(item.id)}
            className="shrink-0 rounded-sm p-0.5 hover:bg-muted-foreground/15 text-muted-foreground"
            title={t("editItem")}
            disabled={isClaimed}
          >
            <Pencil className="h-2.5 w-2.5" />
          </button>
          <button
            type="button"
            onClick={() => onDelete(item.id)}
            className="shrink-0 rounded-sm p-0.5 hover:bg-muted-foreground/15 text-muted-foreground"
            title={t("deleteItem")}
            disabled={isClaimed}
          >
            <X className="h-2.5 w-2.5" />
          </button>
        </>
      ) : null}
    </Reorder.Item>
  )
}

export function MessageQueueDisplay({
  queue,
  onReorder,
  onEdit,
  onDelete,
  onRetry,
  onPauseManual,
  onReleaseOne,
  onResume,
  pausedReason,
  manualReleaseItemId,
  editingItemId,
}: MessageQueueDisplayProps) {
  const t = useTranslations("Folder.chat.messageQueue")
  const manualReview = pausedReason === "manual_review"

  const displayedPausedReason =
    pausedReason === "cancelled_current_turn"
      ? t("pauseReasonCancelled")
      : pausedReason === "dispatch_outcome_unknown"
        ? t("pauseReasonUnknownDispatch")
        : pausedReason

  return (
    <div className="max-h-36 overflow-y-auto pb-1">
      {!pausedReason ? (
        <div className="mb-1 flex justify-end">
          <button
            type="button"
            onClick={onPauseManual}
            className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10px] text-muted-foreground hover:bg-muted hover:text-foreground"
            title={t("pauseManualDescription")}
          >
            <PauseCircle className="h-3 w-3" />
            {t("pauseManual")}
          </button>
        </div>
      ) : null}
      {pausedReason ? (
        <div className="mb-1 flex items-center gap-1.5 rounded-md border border-amber-500/30 bg-amber-500/8 px-2 py-1 text-[10px] text-amber-700 dark:text-amber-300">
          <AlertTriangle className="h-3 w-3 shrink-0" />
          <span
            className="min-w-0 flex-1 truncate"
            title={displayedPausedReason ?? undefined}
          >
            {manualReview
              ? t("manualPaused")
              : t("paused", { reason: displayedPausedReason ?? "" })}
          </span>
          {!queue.some((item) => item.state === "paused") ? (
            <button
              type="button"
              onClick={onResume}
              className="shrink-0 rounded px-1.5 py-0.5 font-medium hover:bg-amber-500/15"
            >
              {t("resumeQueue")}
            </button>
          ) : null}
        </div>
      ) : null}
      {queue.length > 0 ? (
        <Reorder.Group
          as="div"
          axis="y"
          values={queue}
          onReorder={onReorder}
          className="flex flex-col gap-0.5"
        >
          {queue.map((item, index) => (
            <QueueItem
              key={item.id}
              item={item}
              index={index}
              isEditing={editingItemId === item.id}
              onEdit={onEdit}
              onDelete={onDelete}
              onRetry={onRetry}
              onReleaseOne={onReleaseOne}
              manualReview={manualReview}
              releasePending={manualReleaseItemId != null}
            />
          ))}
        </Reorder.Group>
      ) : null}
    </div>
  )
}
