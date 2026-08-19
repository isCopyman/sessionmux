"use client"

import { useMemo, useState } from "react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { AgentIcon } from "@/components/agent-icon"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
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
 * mid-selection). Picks members from the live Session list, then creates the
 * Room on the active workbench and opens it — the same tail as the bar's
 * create-room flow.
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
  const openRoom = useOpenRoom()
  const [query, setQuery] = useState("")
  const [selectedIds, setSelectedIds] = useState<number[]>([])
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
  // Selection order, not list order: the first pick becomes the Room creator.
  const members = useMemo(
    () =>
      selectedIds
        .map((id) => conversationById.get(id))
        .filter(
          (conversation): conversation is NonNullable<typeof conversation> =>
            conversation != null
        ),
    [conversationById, selectedIds]
  )
  const autoTitle = defaultRoomTitle(members, t("createTitle"))

  const handleOpenChange = (next: boolean) => {
    onOpenChange(next)
    if (!next) {
      setQuery("")
      setSelectedIds([])
      setTitleDraft(null)
    }
  }

  const handleCreate = async () => {
    if (members.length < 2) {
      toast.error(t("createNeedTwo"))
      return
    }
    setPending(true)
    try {
      const created = await createCollaborationRoom({
        workbenchId: activeWorkbenchId,
        title: (titleDraft ?? "").trim() || autoTitle,
        memberConversationIds: members.map((member) => member.id),
        createdByConversationId: members[0].id,
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
            <ul className="flex flex-col gap-1">
              {candidates.map((conversation) => {
                const checked = selectedIds.includes(conversation.id)
                return (
                  <li key={conversation.id}>
                    <button
                      type="button"
                      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-muted/60"
                      onClick={() =>
                        setSelectedIds((current) =>
                          current.includes(conversation.id)
                            ? current.filter((id) => id !== conversation.id)
                            : [...current, conversation.id]
                        )
                      }
                    >
                      <Checkbox checked={checked} />
                      <AgentIcon
                        agentType={conversation.agent_type}
                        className="h-3.5 w-3.5"
                      />
                      <span className="min-w-0 flex-1 truncate">
                        {formatConversationTitle(conversation.title) ||
                          t("untitled", { id: conversation.id })}
                      </span>
                    </button>
                  </li>
                )
              })}
            </ul>
          )}
        </ScrollArea>
        <p className="text-[11px] text-muted-foreground">{t("createHint")}</p>
        <DialogFooter>
          <Button variant="outline" onClick={() => handleOpenChange(false)}>
            {t("cancel")}
          </Button>
          <Button
            disabled={selectedIds.length < 2 || pending}
            onClick={() => void handleCreate()}
          >
            {t("createSubmit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
