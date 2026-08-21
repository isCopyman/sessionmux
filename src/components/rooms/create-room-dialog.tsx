"use client"

import { useMemo, useState } from "react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { AgentIcon } from "@/components/agent-icon"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { ScrollArea } from "@/components/ui/scroll-area"
import { toErrorMessage } from "@/lib/app-error"
import { createCollaborationRoom } from "@/lib/api"
import { formatConversationTitle } from "@/lib/conversation-title"
import { useOpenRoom } from "@/lib/open-room"
import { defaultRoomTitle, roomMemberCandidates } from "@/lib/room-create"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useTabStore } from "@/stores/tab-store"

/**
 * Always-visible create-room entry point (the bulk action bar only offers it
 * mid-selection). Picks the initiator Session, creates the Room on the active
 * workbench, and opens it so members can be invited from inside.
 */
export function CreateRoomDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const t = useTranslations("Room")
  const tManage = useTranslations("Folder.sidebar.manageConversations")
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const activeWorkbenchId = useTabStore((state) => state.activeWorkbenchId)
  const activeConversationId = useTabStore((state) => {
    const tab = state.rawTabs.find((item) => item.id === state.activeTabId)
    if (tab == null || tab.kind !== "conversation") return null
    return tab.conversationId
  })
  const openRoom = useOpenRoom()
  const [query, setQuery] = useState("")
  const [selectedId, setSelectedId] = useState<number | null>(
    activeConversationId
  )
  const [titleDraft, setTitleDraft] = useState<string | null>(null)
  const [pending, setPending] = useState(false)

  const candidates = useMemo(
    () => roomMemberCandidates(conversations, query),
    [conversations, query]
  )
  const conversationById = useMemo(
    () =>
      new Map(
        conversations.map((conversation) => [conversation.id, conversation])
      ),
    [conversations]
  )
  const initiator = useMemo(() => {
    if (selectedId == null) return undefined
    const conversation = conversationById.get(selectedId)
    if (conversation == null || conversation.archived_at != null) {
      return undefined
    }
    return conversation
  }, [conversationById, selectedId])
  const members = initiator == null ? [] : [initiator]
  const autoTitle = defaultRoomTitle(members, t("createTitle"), (name) =>
    t("createTitleSolo", { name })
  )

  const handleOpenChange = (next: boolean) => {
    onOpenChange(next)
    if (!next) {
      setQuery("")
      setSelectedId(null)
      setTitleDraft(null)
    }
  }

  const handleCreate = async () => {
    if (initiator == null) {
      toast.error(t("createNeedTwo"))
      return
    }
    setPending(true)
    try {
      const created = await createCollaborationRoom({
        workbenchId: activeWorkbenchId,
        title: (titleDraft ?? "").trim() || autoTitle,
        memberConversationIds: [initiator.id],
        createdByConversationId: initiator.id,
      })
      toast.success(tManage("toastRoomCreated", { title: created.title }))
      handleOpenChange(false)
      await openRoom(created)
    } catch (error) {
      toast.error(t("createFailed", { message: toErrorMessage(error) }))
    } finally {
      setPending(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{t("createTitle")}</DialogTitle>
        </DialogHeader>
        <Input
          value={titleDraft ?? autoTitle}
          onChange={(event) => setTitleDraft(event.target.value)}
          placeholder={t("createNamePlaceholder")}
          aria-label={t("createNamePlaceholder")}
        />
        <Input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t("addMemberSearch")}
        />
        <ScrollArea className="h-56">
          {candidates.length === 0 ? (
            <p className="px-1 py-6 text-center text-xs text-muted-foreground">
              {t("createNoCandidates")}
            </p>
          ) : (
            <RadioGroup
              value={selectedId != null ? String(selectedId) : ""}
              onValueChange={(value) => setSelectedId(Number(value))}
              className="flex flex-col gap-1"
            >
              {candidates.map((conversation) => (
                <Label
                  key={conversation.id}
                  className="flex w-full cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs font-normal hover:bg-muted/60"
                >
                  <RadioGroupItem value={String(conversation.id)} />
                  <AgentIcon
                    agentType={conversation.agent_type}
                    className="h-3.5 w-3.5"
                  />
                  <span className="min-w-0 flex-1 truncate">
                    {formatConversationTitle(conversation.title) ||
                      t("untitled", { id: conversation.id })}
                  </span>
                </Label>
              ))}
            </RadioGroup>
          )}
        </ScrollArea>
        <p className="text-[11px] text-muted-foreground">{t("createHint")}</p>
        <DialogFooter>
          <Button variant="outline" onClick={() => handleOpenChange(false)}>
            {t("cancel")}
          </Button>
          <Button
            disabled={initiator == null || pending}
            onClick={() => void handleCreate()}
          >
            {t("createSubmit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
