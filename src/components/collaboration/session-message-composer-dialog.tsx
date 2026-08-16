"use client"

import { useMemo, useState } from "react"
import { Check, Loader2, Search, Send } from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import { AgentIcon } from "@/components/agent-icon"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Textarea } from "@/components/ui/textarea"
import {
  sendAndInterruptCollaborationMessage,
  sendCollaborationMessage,
} from "@/lib/api"
import { formatConversationTitle } from "@/lib/conversation-title"
import { getAgentLabel } from "@/lib/custom-agents"
import { randomUUID } from "@/lib/utils"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import type { CollaborationInvocationPolicy } from "@/lib/types"
import type { CollaborationDeliveryHint } from "@/lib/types"

const MAX_BODY_BYTES = 1_000_000

interface SessionMessageComposerDialogProps {
  sourceConversationId: number | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SessionMessageComposerDialog({
  sourceConversationId,
  open,
  onOpenChange,
}: SessionMessageComposerDialogProps) {
  const t = useTranslations("Collaboration")
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const folders = useAppWorkspaceStore((state) => state.folders)
  const [query, setQuery] = useState("")
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [body, setBody] = useState("")
  const [invocationPolicy, setInvocationPolicy] =
    useState<CollaborationInvocationPolicy>("store_only")
  const [deliveryHint, setDeliveryHint] =
    useState<CollaborationDeliveryHint>("default")
  const [interruptCurrentTask, setInterruptCurrentTask] = useState(false)
  const [sending, setSending] = useState(false)

  const folderById = useMemo(
    () => new Map(folders.map((folder) => [folder.id, folder])),
    [folders]
  )
  const candidates = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase()
    return conversations
      .filter(
        (conversation) =>
          conversation.id !== sourceConversationId &&
          conversation.kind !== "loop"
      )
      .filter((conversation) => {
        if (!normalized) return true
        const folder = folderById.get(conversation.folder_id)
        return [
          formatConversationTitle(conversation.title),
          getAgentLabel(conversation.agent_type),
          folder?.name,
          folder?.path,
          String(conversation.id),
        ].some((value) => value?.toLocaleLowerCase().includes(normalized))
      })
      .sort(
        (a, b) =>
          new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime()
      )
  }, [conversations, folderById, query, sourceConversationId])

  const bodyBytes = useMemo(() => new Blob([body]).size, [body])
  const bodyTooLarge = bodyBytes > MAX_BODY_BYTES
  const canSend =
    sourceConversationId != null &&
    selected.size > 0 &&
    (!interruptCurrentTask || selected.size === 1) &&
    body.trim().length > 0 &&
    !bodyTooLarge &&
    !sending

  const reset = () => {
    setQuery("")
    setSelected(new Set())
    setBody("")
    setInvocationPolicy("store_only")
    setDeliveryHint("default")
    setInterruptCurrentTask(false)
  }

  const setOpen = (next: boolean) => {
    if (!next && !sending) reset()
    onOpenChange(next)
  }

  const toggleTarget = (conversationId: number) => {
    setSelected((current) => {
      if (interruptCurrentTask) {
        return current.has(conversationId)
          ? new Set()
          : new Set([conversationId])
      }
      const next = new Set(current)
      if (next.has(conversationId)) next.delete(conversationId)
      else if (next.size < 16) next.add(conversationId)
      return next
    })
  }

  const handleSend = async () => {
    if (!canSend || sourceConversationId == null) return
    setSending(true)
    try {
      if (interruptCurrentTask) {
        const targetConversationId = [...selected][0]
        const result = await sendAndInterruptCollaborationMessage({
          message: {
            sourceConversationId,
            targetConversationIds: [targetConversationId],
            body,
            clientDedupeId: randomUUID(),
            invocationPolicy: "invoke_when_idle",
            deliveryHint: "default",
            expectsReply: false,
            urgency: "normal",
          },
          interruptClientDedupeId: randomUUID(),
          reason: "User explicitly requested stop current task and send",
        })
        if (result.interruptError || !result.interrupt) {
          toast.warning(t("interruptUnavailableQueued"))
        } else if (result.interrupt.operation.state === "failed") {
          toast.warning(t("interruptFailedQueued"))
        } else {
          toast.success(t("interruptRequested"))
        }
        reset()
        onOpenChange(false)
        return
      }
      const result = await sendCollaborationMessage({
        sourceConversationId,
        targetConversationIds: [...selected],
        body,
        clientDedupeId: randomUUID(),
        invocationPolicy,
        deliveryHint,
        expectsReply: false,
        urgency: "normal",
      })
      const failed = result.deliveries.filter(
        (delivery) => delivery.state === "failed"
      ).length
      if (failed > 0) {
        toast.warning(
          t("sendPartial", {
            sent: result.deliveries.length - failed,
            failed,
          })
        )
      } else {
        toast.success(t("sendSuccess", { count: result.deliveries.length }))
      }
      reset()
      onOpenChange(false)
    } catch (error) {
      console.error("[collaboration] send:", error)
      toast.error(t("sendFailed"))
    } finally {
      setSending(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="flex max-h-[min(42rem,85vh)] max-w-xl flex-col overflow-hidden">
        <DialogHeader>
          <DialogTitle>{t("sendTitle")}</DialogTitle>
          <DialogDescription>
            {invocationPolicy === "store_only"
              ? t("storeOnlyDescription")
              : interruptCurrentTask
                ? t("interruptDescription")
                : deliveryHint === "steer_if_supported"
                  ? t("steerIfSupportedDescription")
                  : t("invokeWhenIdleDescription")}
          </DialogDescription>
        </DialogHeader>

        <div
          role="radiogroup"
          aria-label={t("deliveryMode")}
          className="grid grid-cols-2 gap-1 rounded-lg bg-muted p-1 sm:grid-cols-4"
        >
          <Button
            type="button"
            role="radio"
            aria-checked={invocationPolicy === "store_only"}
            variant={invocationPolicy === "store_only" ? "secondary" : "ghost"}
            size="sm"
            onClick={() => {
              setInvocationPolicy("store_only")
              setDeliveryHint("default")
              setInterruptCurrentTask(false)
            }}
          >
            {t("deliverOnly")}
          </Button>
          <Button
            type="button"
            role="radio"
            aria-checked={
              invocationPolicy === "invoke_when_idle" &&
              deliveryHint === "default" &&
              !interruptCurrentTask
            }
            variant={
              invocationPolicy === "invoke_when_idle" &&
              deliveryHint === "default" &&
              !interruptCurrentTask
                ? "secondary"
                : "ghost"
            }
            size="sm"
            onClick={() => {
              setInvocationPolicy("invoke_when_idle")
              setDeliveryHint("default")
              setInterruptCurrentTask(false)
            }}
          >
            {t("invokeWhenIdle")}
          </Button>
          <Button
            type="button"
            role="radio"
            aria-checked={deliveryHint === "steer_if_supported"}
            variant={
              deliveryHint === "steer_if_supported" ? "secondary" : "ghost"
            }
            size="sm"
            onClick={() => {
              setInvocationPolicy("invoke_when_idle")
              setDeliveryHint("steer_if_supported")
              setInterruptCurrentTask(false)
            }}
          >
            {t("steerIfSupported")}
          </Button>
          <Button
            type="button"
            role="radio"
            aria-checked={interruptCurrentTask}
            variant={interruptCurrentTask ? "secondary" : "ghost"}
            size="sm"
            onClick={() => {
              setInvocationPolicy("invoke_when_idle")
              setDeliveryHint("default")
              setInterruptCurrentTask(true)
              setSelected((current) => {
                const first = current.values().next().value
                return first == null ? new Set() : new Set([first])
              })
            }}
          >
            {t("interruptCurrentTask")}
          </Button>
        </div>

        <div className="relative">
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
          />
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("targetSearchPlaceholder")}
            className="pl-8"
          />
        </div>

        <ScrollArea className="min-h-32 flex-1 rounded-md border" y="scroll">
          <div className="divide-y">
            {candidates.length === 0 ? (
              <p className="px-3 py-6 text-center text-sm text-muted-foreground">
                {t("noTargets")}
              </p>
            ) : (
              candidates.map((conversation) => {
                const checked = selected.has(conversation.id)
                const folder = folderById.get(conversation.folder_id)
                return (
                  <button
                    key={conversation.id}
                    type="button"
                    onClick={() => toggleTarget(conversation.id)}
                    className="flex w-full items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-muted/60"
                  >
                    <span
                      aria-hidden="true"
                      className={`flex h-4 w-4 shrink-0 items-center justify-center rounded-sm border ${
                        checked
                          ? "border-primary bg-primary text-primary-foreground"
                          : "border-input"
                      }`}
                    >
                      {checked ? <Check className="h-3 w-3" /> : null}
                    </span>
                    <AgentIcon
                      agentType={conversation.agent_type}
                      className="h-4 w-4 shrink-0"
                    />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium">
                        {formatConversationTitle(conversation.title) ||
                          t("untitled", { id: conversation.id })}
                      </span>
                      <span className="block truncate text-xs text-muted-foreground">
                        {getAgentLabel(conversation.agent_type)}
                        {folder ? ` · ${folder.name || folder.path}` : ""}
                      </span>
                    </span>
                    {checked ? (
                      <Check className="h-4 w-4 shrink-0 text-primary" />
                    ) : null}
                  </button>
                )
              })
            )}
          </div>
        </ScrollArea>

        <div className="space-y-1.5">
          <Textarea
            value={body}
            onChange={(event) => setBody(event.target.value)}
            placeholder={t("bodyPlaceholder")}
            rows={5}
            autoFocus
          />
          <div className="flex justify-between text-xs text-muted-foreground">
            <span>{t("selectedCount", { count: selected.size })}</span>
            <span className={bodyTooLarge ? "text-destructive" : undefined}>
              {bodyTooLarge
                ? t("bodyTooLarge")
                : t("bodySize", { size: bodyBytes })}
            </span>
          </div>
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => setOpen(false)}
            disabled={sending}
          >
            {t("cancel")}
          </Button>
          <Button onClick={handleSend} disabled={!canSend}>
            {sending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Send className="h-4 w-4" />
            )}
            {t("send")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
