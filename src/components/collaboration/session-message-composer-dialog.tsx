"use client"

import { useMemo, useState } from "react"
import { Check, Loader2, Search, Send } from "lucide-react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"
import { AgentIcon } from "@/components/agent-icon"
import { ConversationStatusDot } from "@/components/conversations/conversation-status-dot"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
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
import { sendCollaborationMessage } from "@/lib/api"
import { formatConversationTitle } from "@/lib/conversation-title"
import { getAgentLabel } from "@/lib/custom-agents"
import { randomUUID } from "@/lib/utils"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { STATUS_ORDER } from "@/lib/types"
import type {
  CollaborationDeliveryHint,
  CollaborationInvocationPolicy,
  ConversationStatus,
} from "@/lib/types"

const MAX_BODY_BYTES = 1_000_000

interface SessionMessageComposerDialogProps {
  sourceConversationId: number | null
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Stable Session id preselected by a contextual reply action. */
  initialTargetConversationId?: number | null
  /** Structured `@` Session badges from the chat composer. */
  initialTargetConversationIds?: number[]
  /** Prefill from the chat composer after Session badges are stripped. */
  initialBody?: string
  /** Immutable collaboration event being answered, when this is a reply. */
  replyToEventId?: string | null
  /** Fired after the event is persisted, including a partial fan-out. */
  onSent?: (info: { count: number }) => void
}

export function SessionMessageComposerDialog({
  sourceConversationId,
  open,
  onOpenChange,
  initialTargetConversationId = null,
  initialTargetConversationIds = [],
  initialBody = "",
  replyToEventId = null,
  onSent,
}: SessionMessageComposerDialogProps) {
  const t = useTranslations("Collaboration")
  const tStatus = useTranslations("Folder.statusLabels")
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const folders = useAppWorkspaceStore((state) => state.folders)
  const presetTargets = useMemo(() => {
    const ids = [...initialTargetConversationIds]
    if (initialTargetConversationId != null)
      ids.push(initialTargetConversationId)
    return [...new Set(ids.filter((id) => Number.isInteger(id) && id > 0))]
  }, [initialTargetConversationId, initialTargetConversationIds])
  const [query, setQuery] = useState("")
  const [selected, setSelected] = useState<Set<number>>(
    () => new Set(presetTargets)
  )
  const [subject, setSubject] = useState("")
  const [body, setBody] = useState(initialBody)
  const [priority, setPriority] = useState<"high" | "normal">("high")
  const [deliveryHint, setDeliveryHint] =
    useState<CollaborationDeliveryHint>("default")
  const [expectsReply, setExpectsReply] = useState(false)
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
          conversation.kind !== "loop" &&
          (replyToEventId == null ||
            conversation.id === initialTargetConversationId)
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
  }, [
    conversations,
    folderById,
    initialTargetConversationId,
    query,
    replyToEventId,
    sourceConversationId,
  ])

  const duplicateTitles = useMemo(() => {
    const counts = new Map<string, number>()
    for (const conversation of candidates) {
      const title =
        formatConversationTitle(conversation.title) ||
        t("untitled", { id: conversation.id })
      counts.set(title, (counts.get(title) ?? 0) + 1)
    }
    return new Set(
      [...counts.entries()]
        .filter(([, count]) => count > 1)
        .map(([title]) => title)
    )
  }, [candidates, t])

  const bodyBytes = useMemo(() => new Blob([body]).size, [body])
  const bodyTooLarge = bodyBytes > MAX_BODY_BYTES
  const canSend =
    sourceConversationId != null &&
    selected.size > 0 &&
    subject.trim().length > 0 &&
    subject.trim().length <= 120 &&
    body.trim().length > 0 &&
    !bodyTooLarge &&
    !sending

  const reset = () => {
    setQuery("")
    setSelected(new Set(presetTargets))
    setSubject("")
    setBody(initialBody)
    setPriority("high")
    setDeliveryHint("default")
    setExpectsReply(false)
  }

  const setOpen = (next: boolean) => {
    if (!next && !sending) reset()
    onOpenChange(next)
  }

  const toggleTarget = (conversationId: number) => {
    if (replyToEventId != null) return
    setSelected((current) => {
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
      const result = await sendCollaborationMessage({
        sourceConversationId,
        targetConversationIds: [...selected],
        subject: subject.trim(),
        body,
        clientDedupeId: randomUUID(),
        invocationPolicy: (priority === "high"
          ? "invoke_when_idle"
          : "store_only") satisfies CollaborationInvocationPolicy,
        deliveryHint,
        expectsReply,
        urgency: priority === "high" ? "urgent" : "normal",
        replyToEventId,
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
      onSent?.({ count: result.deliveries.length })
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
      <DialogContent
        data-collaboration-composer=""
        className="flex max-h-[min(42rem,85vh)] max-w-xl flex-col overflow-hidden"
      >
        <DialogHeader>
          <DialogTitle>{t("sendTitle")}</DialogTitle>
          <DialogDescription>
            {priority === "high"
              ? t("priorityHighDescription")
              : t("priorityNormalDescription")}
          </DialogDescription>
        </DialogHeader>

        <div
          role="radiogroup"
          aria-label={t("letterPriority")}
          className="grid grid-cols-2 gap-1 rounded-lg bg-muted p-1"
        >
          <Button
            type="button"
            role="radio"
            aria-checked={priority === "high"}
            variant={priority === "high" ? "secondary" : "ghost"}
            size="sm"
            onClick={() => setPriority("high")}
          >
            {t("priorityHigh")}
          </Button>
          <Button
            type="button"
            role="radio"
            aria-checked={priority === "normal"}
            variant={priority === "normal" ? "secondary" : "ghost"}
            size="sm"
            onClick={() => setPriority("normal")}
          >
            {t("priorityNormal")}
          </Button>
        </div>

        {replyToEventId == null ? (
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
        ) : null}

        <ScrollArea className="min-h-32 flex-1 rounded-md border" y="scroll">
          <div className="divide-y">
            {candidates.length === 0 ? (
              <p className="px-3 py-6 text-center text-sm text-muted-foreground">
                {t("noTargets")}
              </p>
            ) : (
              candidates.map((conversation) => {
                const checked = selected.has(conversation.id)
                const title =
                  formatConversationTitle(conversation.title) ||
                  t("untitled", { id: conversation.id })
                const displayTitle = duplicateTitles.has(title)
                  ? `${title} #${conversation.id}`
                  : title
                const folder = folderById.get(conversation.folder_id)
                const runtimeStatus = STATUS_ORDER.includes(
                  conversation.status as ConversationStatus
                )
                  ? (conversation.status as ConversationStatus)
                  : null
                return (
                  <button
                    key={conversation.id}
                    type="button"
                    data-collaboration-target={conversation.id}
                    onClick={() => toggleTarget(conversation.id)}
                    disabled={replyToEventId != null}
                    className="flex w-full items-center gap-3 px-3 py-2 text-left transition-colors enabled:hover:bg-muted/60"
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
                    <ConversationStatusDot
                      status={runtimeStatus}
                      size="sm"
                      title={
                        runtimeStatus
                          ? tStatus(runtimeStatus)
                          : conversation.status || undefined
                      }
                    />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium">
                        {displayTitle}
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
          <Input
            value={subject}
            onChange={(event) => setSubject(event.target.value)}
            placeholder={t("letterTitlePlaceholder")}
            maxLength={120}
            autoFocus
            data-collaboration-subject=""
          />
          <Textarea
            value={body}
            onChange={(event) => setBody(event.target.value)}
            placeholder={t("bodyPlaceholder")}
            rows={5}
            data-collaboration-body=""
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

        <label className="flex cursor-pointer items-start gap-2.5 rounded-md border px-3 py-2.5">
          <Checkbox
            checked={expectsReply}
            onCheckedChange={(checked) => setExpectsReply(checked === true)}
            aria-label={t("requestReply")}
            className="mt-0.5"
          />
          <span className="min-w-0">
            <span className="block text-sm font-medium">
              {t("requestReply")}
            </span>
            <span className="block text-xs text-muted-foreground">
              {t("requestReplyDescription")}
            </span>
          </span>
        </label>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => setOpen(false)}
            disabled={sending}
          >
            {t("cancel")}
          </Button>
          <Button
            onClick={() => void handleSend()}
            disabled={!canSend}
            data-collaboration-submit=""
          >
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
