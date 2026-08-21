"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { useTranslations } from "next-intl"
import { AlertTriangle, RefreshCw } from "lucide-react"
import { toast } from "sonner"
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
import { Button } from "@/components/ui/button"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { useConnection } from "@/hooks/use-connection"
import { cn } from "@/lib/utils"

/**
 * Per-conversation banner shown at the top of a session panel when the agent's
 * effective settings changed AFTER the session spawned, so the running process
 * is still on its launch-time config. In the tiled layout each stale session
 * renders its own banner, so the user can spot and resolve them one by one.
 *
 * Behaviour:
 * - Owners only — viewers and delegation children don't own the backend
 *   process, so reconnecting isn't theirs to do.
 * - Restarts immediately when no turn or queued prompt can be disrupted.
 * - Otherwise forces a choice between restarting now and restarting at the
 *   next idle moment. The stale state cannot be dismissed without applying it.
 * - Responsive via container queries: in a narrow panel (small screen or a
 *   thin tiled column) the text and the actions stack; they sit on one row
 *   once the panel is wide enough.
 *
 * Returns null (no layout impact) when there's nothing to show.
 */
export function SessionConfigStaleBanner({
  contextKey,
  queueDepth,
  queueHydrated,
}: {
  contextKey: string
  queueDepth: number
  queueHydrated: boolean
}) {
  const t = useTranslations("Folder.chat.configStale")
  const {
    configStale,
    configStaleKind,
    isViewer,
    isDelegationChild,
    status,
    reapplyConfig,
  } = useConnection(contextKey)
  // Our own "reconnect in flight" flag. Kept distinct from the connection's
  // `connecting` status because `reapplyConfig` briefly disconnects first.
  const [reconnecting, setReconnecting] = useState(false)
  const [dialogOpen, setDialogOpen] = useState(false)
  const restartDeferredRef = useRef(false)
  const handledStaleRef = useRef(false)

  const handleRestart = useCallback(async () => {
    setReconnecting(true)
    try {
      const reconnected = await reapplyConfig()
      if (reconnected) {
        setDialogOpen(false)
        restartDeferredRef.current = false
        toast.success(t("applied"))
      }
    } catch (error) {
      toast.error(t("reconnectFailed"), {
        description: error instanceof Error ? error.message : String(error),
      })
    } finally {
      setReconnecting(false)
    }
  }, [reapplyConfig, t])

  useEffect(() => {
    if (!configStale) {
      handledStaleRef.current = false
      restartDeferredRef.current = false
      setDialogOpen(false)
      return
    }
    if (
      isViewer ||
      isDelegationChild ||
      !queueHydrated ||
      (status !== "connected" && status !== "prompting")
    ) {
      return
    }

    if (restartDeferredRef.current) {
      if (status === "connected" && queueDepth === 0) {
        restartDeferredRef.current = false
        void handleRestart()
      }
      return
    }

    if (handledStaleRef.current) return

    // Claim only the false -> true stale transition. reapplyConfig re-emits
    // connection states while stale is still true; those states must not start
    // another restart. The claim resets only after stale becomes false.
    handledStaleRef.current = true
    if (status === "prompting" || queueDepth > 0) {
      setDialogOpen(true)
    } else {
      void handleRestart()
    }
  }, [
    configStale,
    handleRestart,
    isDelegationChild,
    isViewer,
    queueDepth,
    queueHydrated,
    status,
  ])

  // Owners only: viewers and delegation children don't own the backend process,
  // so restarting isn't theirs to do.
  if (!configStale || isViewer || isDelegationChild) return null

  const turnInFlight = status === "prompting"
  // Spinner while our reconnect is in flight OR the connection is
  // (re)establishing — `connecting` covers the reconnect `reapplyConfig` fires.
  const busy = reconnecting || status === "connecting"
  const actionDisabled =
    !queueHydrated || turnInFlight || queueDepth > 0 || busy

  const title =
    configStaleKind === "model_provider"
      ? t("modelProviderTitle")
      : t("agentConfigTitle")

  return (
    <div className="@container border-b border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-1.5 @lg:flex-row @lg:items-center @lg:gap-2">
        <div className="flex min-w-0 flex-1 items-start gap-2">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400 @lg:mt-0" />
          <div className="min-w-0 leading-snug">
            <span className="font-medium">{title}</span>{" "}
            <span className="text-amber-700/80 dark:text-amber-300/80">
              {t("description")}
            </span>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1 self-end @lg:self-auto">
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                {/* Wrapper span so the tooltip still fires while the button is
                    disabled (disabled elements don't emit pointer events). */}
                <span>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-7 gap-1.5 border-amber-500/40 bg-transparent text-amber-700 hover:bg-amber-500/20 hover:text-amber-800 dark:text-amber-300 dark:hover:text-amber-200"
                    disabled={actionDisabled}
                    onClick={() => void handleRestart()}
                  >
                    <RefreshCw
                      className={cn("h-3.5 w-3.5", busy && "animate-spin")}
                    />
                    {busy ? t("reconnecting") : t("reconnect")}
                  </Button>
                </span>
              </TooltipTrigger>
              {actionDisabled && !busy ? (
                <TooltipContent>
                  {t("reconnectDisabledDuringTurn")}
                </TooltipContent>
              ) : null}
            </Tooltip>
          </TooltipProvider>
        </div>
      </div>
      <AlertDialog open={dialogOpen} onOpenChange={() => {}}>
        <AlertDialogContent onEscapeKeyDown={(event) => event.preventDefault()}>
          <AlertDialogHeader>
            <AlertDialogTitle>{title}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("restartDescription", { queueDepth })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel
              disabled={reconnecting}
              onClick={() => {
                restartDeferredRef.current = true
                setDialogOpen(false)
              }}
            >
              {t("restartAfterTurn")}
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={reconnecting}
              onClick={() => void handleRestart()}
            >
              {t("restartNow")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
