"use client"

import { useState } from "react"
import { useTranslations } from "next-intl"

import { SessionSelect } from "@/components/shared/session-select"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { toErrorMessage } from "@/lib/app-error"
import type { DbConversationSummary, WorkTask } from "@/lib/types"

interface TaskAssignSessionDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  task: WorkTask | null
  sessions: readonly DbConversationSummary[]
  onSubmit: (conversationId: number) => Promise<void>
  onCreateSession: () => void
}

/**
 * Assigns a neutral board card to one existing persistent Session. The picker
 * intentionally lists only Sessions supplied by the page: the root project
 * and its worktrees. The backend repeats that boundary check transactionally.
 */
export function TaskAssignSessionDialog({
  open,
  onOpenChange,
  task,
  sessions,
  onSubmit,
  onCreateSession,
}: TaskAssignSessionDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open ? (
        <TaskAssignSessionBody
          task={task}
          sessions={sessions}
          onSubmit={onSubmit}
          onCreateSession={onCreateSession}
          onCancel={() => onOpenChange(false)}
        />
      ) : null}
    </Dialog>
  )
}

/**
 * The body is mounted only while the dialog is open. This gives every opening
 * a fresh selection/error state without an effect that races the first render.
 */
function TaskAssignSessionBody({
  task,
  sessions,
  onSubmit,
  onCreateSession,
  onCancel,
}: {
  task: WorkTask | null
  sessions: readonly DbConversationSummary[]
  onSubmit: (conversationId: number) => Promise<void>
  onCreateSession: () => void
  onCancel: () => void
}) {
  const t = useTranslations("Tasks")
  const [conversationId, setConversationId] = useState<number | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async () => {
    if (conversationId == null || submitting) return
    setSubmitting(true)
    setError(null)
    try {
      await onSubmit(conversationId)
    } catch (cause) {
      setError(toErrorMessage(cause))
      setSubmitting(false)
    }
  }

  return (
    <DialogContent className="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{t("assignSessionTitle")}</DialogTitle>
        <DialogDescription>
          {t("assignSessionDescription", { title: task?.title ?? "" })}
        </DialogDescription>
      </DialogHeader>

      <div className="space-y-2 py-2">
        <SessionSelect
          sessions={sessions}
          value={conversationId}
          onChange={setConversationId}
          placeholder={t("assignSessionPlaceholder")}
          searchPlaceholder={t("assignSessionSearch")}
          emptyLabel={t("assignSessionEmpty")}
          title={t("assignSessionField")}
          className="w-full max-w-none justify-between"
        />
        {error ? (
          <p className="text-sm text-destructive" role="alert">
            {error}
          </p>
        ) : null}
      </div>

      <DialogFooter>
        <Button
          type="button"
          variant="secondary"
          onClick={onCreateSession}
          disabled={submitting}
        >
          {t("assignSessionCreateNew")}
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={onCancel}
          disabled={submitting}
        >
          {t("cancel")}
        </Button>
        <Button
          type="button"
          onClick={() => void submit()}
          disabled={conversationId == null || submitting}
        >
          {submitting ? t("assignSessionSubmitting") : t("assignSessionSubmit")}
        </Button>
      </DialogFooter>
    </DialogContent>
  )
}
