"use client"

import { useMemo, useState } from "react"
import { ListTodo } from "lucide-react"
import { useTranslations } from "next-intl"

import { StatusChip } from "@/components/tasks/task-card"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useOptionalTasksView } from "@/contexts/tasks-view-context"
import { useOptionalWorkbenchRoute } from "@/contexts/workbench-route-context"
import { requestOpenTaskDetail } from "@/lib/task-compose-events"
import type { WorkTask } from "@/lib/types"

interface SessionTaskBannerProps {
  conversationId: number | null
}

const terminalStatuses = new Set(["done", "canceled"])

function sortResponsibleTasks(tasks: readonly WorkTask[]): WorkTask[] {
  return [...tasks].sort((a, b) => {
    const attentionA = a.task_status === "blocked" || a.task_status === "review"
    const attentionB = b.task_status === "blocked" || b.task_status === "review"
    if (attentionA !== attentionB) return attentionA ? -1 : 1
    const activeA = a.task_status === "in_progress"
    const activeB = b.task_status === "in_progress"
    if (activeA !== activeB) return activeA ? -1 : 1
    return b.updated_at.localeCompare(a.updated_at)
  })
}

/**
 * A high-frequency task entry for one Session. Tasks intentionally live next
 * to the mailbox strip instead of inside the low-frequency metadata dialog.
 * The board remains the only task-detail surface; this panel is a projection.
 */
export function SessionTaskBanner({ conversationId }: SessionTaskBannerProps) {
  const t = useTranslations("Tasks")
  const tasksView = useOptionalTasksView()
  const workbenchRoute = useOptionalWorkbenchRoute()
  const [open, setOpen] = useState(false)

  const responsibleTasks = useMemo(
    () =>
      sortResponsibleTasks(
        (tasksView?.tasks ?? []).filter(
          (task) =>
            conversationId != null &&
            task.conversation_id === conversationId &&
            task.archived_at == null &&
            !terminalStatuses.has(task.task_status)
        )
      ),
    [conversationId, tasksView?.tasks]
  )

  if (conversationId == null || responsibleTasks.length === 0) return null

  const openTask = (taskId: number) => {
    requestOpenTaskDetail(taskId)
    setOpen(false)
    workbenchRoute?.setRoute("tasks")
  }

  return (
    <section
      data-session-task-banner=""
      className="border-b border-border/60 bg-background/80"
    >
      <div className="flex items-center px-3 py-1">
        <button
          type="button"
          className="inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-md px-1.5 py-0.5 text-left text-xs transition-colors hover:bg-muted/60"
          onClick={() => setOpen(true)}
        >
          <ListTodo className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          <span className="font-medium">{t("title")}</span>
          <span className="rounded-full bg-primary/10 px-1.5 py-px text-[10px] font-semibold text-primary">
            {responsibleTasks.length}
          </span>
          <span className="truncate text-muted-foreground">
            {responsibleTasks[0]?.title}
          </span>
        </button>
      </div>

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("title")}</DialogTitle>
            <DialogDescription className="sr-only">
              {t("title")}: {responsibleTasks.length}
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-[min(60vh,30rem)] space-y-1 overflow-y-auto">
            {responsibleTasks.map((task) => (
              <button
                key={task.id}
                type="button"
                onClick={() => openTask(task.id)}
                className="flex w-full min-w-0 items-center gap-2 rounded-lg border border-transparent px-3 py-2 text-left transition-colors hover:border-border hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                  {task.title}
                </span>
                <StatusChip task={task} className="max-w-[9rem]" />
              </button>
            ))}
          </div>
        </DialogContent>
      </Dialog>
    </section>
  )
}
