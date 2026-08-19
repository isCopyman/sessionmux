"use client"

import { useEffect, useState } from "react"
import {
  PauseIcon,
  PencilIcon,
  PlayIcon,
  PlusIcon,
  RotateCcwIcon,
  SaveIcon,
  TimerIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react"
import { useTranslations } from "next-intl"
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
import { useSessionTimers } from "@/hooks/use-session-timers"
import {
  formatCompactCountdown,
  nextFireTimer,
} from "@/lib/session-timer-next-fire"
import { cn } from "@/lib/utils"
import type { SessionTimer } from "@/lib/types"

/**
 * Lightweight Session continuation control. The frontend never counts down:
 * the backend observes ACP TurnComplete and owns all scheduling.
 */
export function SessionTimers({
  conversationId,
}: {
  conversationId?: number | null
}) {
  const t = useTranslations("Folder.chat.sessionTimers")
  const { timers, create, update, resetDelay, remove } =
    useSessionTimers(conversationId)
  const [open, setOpen] = useState(false)
  const [promptText, setPromptText] = useState("")
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editingText, setEditingText] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<SessionTimer | null>(null)
  // Ticks the pill's countdown estimate. The backend is the trigger authority;
  // this only repaints the "~in 3m" hint (see session-timer-next-fire).
  const [now, setNow] = useState(() => Date.now())
  const nextFire = nextFireTimer(timers)
  const hasNextFire = nextFire != null
  useEffect(() => {
    if (!hasNextFire) return
    const interval = setInterval(() => setNow(Date.now()), 15_000)
    return () => clearInterval(interval)
  }, [hasNextFire])

  if (conversationId == null) return null

  const submit = () => {
    const prompt = promptText.trim()
    if (!prompt) {
      setError(t("errors.emptyPrompt"))
      return
    }
    create({ promptText: prompt, idleGraceSecs: 2 })
    setPromptText("")
    setError(null)
  }

  const startEdit = (id: string, prompt: string) => {
    setEditingId(id)
    setEditingText(prompt)
  }

  const saveEdit = () => {
    const prompt = editingText.trim()
    if (!editingId || !prompt) return
    update(editingId, { promptText: prompt })
    setEditingId(null)
    setEditingText("")
  }

  // The collapsed pill says what it is waiting for, not just how many: the
  // next estimated fire rides alongside the count ("2 timers · ~3m"), with the
  // full prompt in the tooltip. An overdue estimate (backend still waiting for
  // an idle window) reads "due" instead of a frozen "0s".
  const nextFireIn = nextFire ? nextFire.at - now : null
  const pillTitle = nextFire
    ? t("nextFireTitle", {
        time: formatCompactCountdown(Math.max(0, nextFireIn ?? 0)),
        prompt:
          nextFire.timer.promptText.length > 40
            ? `${nextFire.timer.promptText.slice(0, 40)}…`
            : nextFire.timer.promptText,
      })
    : t("title")

  return (
    <div className="pb-1">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        className={cn(
          "inline-flex items-center gap-1 rounded-md border px-1.5 py-1 text-[10px] leading-none",
          "border-border/70 bg-muted/40 text-muted-foreground transition-colors hover:bg-muted",
          timers.some((timer) => timer.enabled) &&
            "border-primary/40 text-primary",
          open && "bg-primary/5 text-foreground"
        )}
        title={pillTitle}
      >
        <TimerIcon className="h-3 w-3" />
        {timers.length > 0 ? (
          <span>
            {t("count", { count: timers.length })}
            {nextFire ? (
              <span className="text-muted-foreground/80">
                {" · "}
                {nextFireIn != null && nextFireIn > 0
                  ? t("nextIn", {
                      time: formatCompactCountdown(nextFireIn),
                    })
                  : t("dueShort")}
              </span>
            ) : null}
          </span>
        ) : (
          <span>{t("title")}</span>
        )}
      </button>

      {open && (
        <div className="mt-1 rounded-md border border-border/70 bg-muted/20 p-2 text-xs">
          {timers.length > 0 && (
            <ul className="mb-2 flex flex-col gap-1">
              {timers.map((timer) => (
                <li
                  key={timer.id}
                  className={cn(
                    "rounded-md border border-border/60 bg-background/60 px-1.5 py-1",
                    !timer.enabled && "opacity-60"
                  )}
                >
                  {editingId === timer.id ? (
                    <div className="flex items-start gap-1">
                      <textarea
                        autoFocus
                        value={editingText}
                        onChange={(event) => setEditingText(event.target.value)}
                        rows={3}
                        aria-label={t("promptLabel")}
                        className="min-w-0 flex-1 resize-y rounded border border-border/60 bg-background px-2 py-1 text-[11px] outline-none focus-visible:ring-1 focus-visible:ring-ring"
                      />
                      <button
                        type="button"
                        onClick={saveEdit}
                        disabled={!editingText.trim()}
                        className="rounded-sm p-1 text-muted-foreground hover:bg-muted disabled:opacity-40"
                        title={t("save")}
                      >
                        <SaveIcon className="h-3 w-3" />
                      </button>
                      <button
                        type="button"
                        onClick={() => setEditingId(null)}
                        className="rounded-sm p-1 text-muted-foreground hover:bg-muted"
                        title={t("cancel")}
                      >
                        <XIcon className="h-3 w-3" />
                      </button>
                    </div>
                  ) : (
                    <>
                      <div className="flex items-center gap-1.5">
                        <span
                          className={cn(
                            "h-1.5 w-1.5 shrink-0 rounded-full",
                            timer.enabled
                              ? "bg-primary"
                              : "bg-muted-foreground/50"
                          )}
                        />
                        <span
                          className="min-w-0 flex-1 truncate text-foreground/80"
                          title={timer.promptText}
                        >
                          {timer.promptText}
                        </span>
                        <button
                          type="button"
                          onClick={() => startEdit(timer.id, timer.promptText)}
                          className="shrink-0 rounded-sm p-0.5 text-muted-foreground hover:bg-muted-foreground/15"
                          title={t("edit")}
                        >
                          <PencilIcon className="h-3 w-3" />
                        </button>
                        <button
                          type="button"
                          onClick={() => resetDelay(timer.id)}
                          className="shrink-0 rounded-sm p-0.5 text-muted-foreground hover:bg-muted-foreground/15"
                          title={t("resetDelay")}
                        >
                          <RotateCcwIcon className="h-3 w-3" />
                        </button>
                        <button
                          type="button"
                          onClick={() =>
                            update(timer.id, { enabled: !timer.enabled })
                          }
                          className="shrink-0 rounded-sm p-0.5 text-muted-foreground hover:bg-muted-foreground/15"
                          title={timer.enabled ? t("pause") : t("resume")}
                        >
                          {timer.enabled ? (
                            <PauseIcon className="h-3 w-3" />
                          ) : (
                            <PlayIcon className="h-3 w-3" />
                          )}
                        </button>
                        <button
                          type="button"
                          onClick={() => setDeleteTarget(timer)}
                          className="shrink-0 rounded-sm p-0.5 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                          title={t("delete")}
                        >
                          <Trash2Icon className="h-3 w-3" />
                        </button>
                      </div>
                      {timer.strikeCount > 0 ? (
                        <p className="mt-1 text-[10px] leading-relaxed text-muted-foreground">
                          {t("delayGrew")}
                        </p>
                      ) : null}
                    </>
                  )}
                </li>
              ))}
            </ul>
          )}

          <div className="flex flex-col gap-1.5">
            <textarea
              value={promptText}
              onChange={(event) => setPromptText(event.target.value)}
              rows={3}
              placeholder={t("promptPlaceholder")}
              aria-label={t("promptLabel")}
              className="resize-y rounded border border-border/60 bg-background px-2 py-1 text-[11px] outline-none focus-visible:ring-1 focus-visible:ring-ring"
            />
            <p className="text-[10px] leading-relaxed text-muted-foreground">
              {t("fileHint")}
            </p>
            {error && <p className="text-[11px] text-destructive">{error}</p>}
            <button
              type="button"
              onClick={submit}
              className="inline-flex items-center gap-1 self-start rounded-md border border-border/60 px-2 py-0.5 text-[11px] font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            >
              <PlusIcon className="h-3 w-3" />
              {t("add")}
            </button>
          </div>
        </div>
      )}

      <AlertDialog
        open={deleteTarget != null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setDeleteTarget(null)
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("deleteTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("deleteDescription", {
                prompt: deleteTarget?.promptText ?? "",
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (deleteTarget) remove(deleteTarget.id)
                setDeleteTarget(null)
              }}
            >
              {t("delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
