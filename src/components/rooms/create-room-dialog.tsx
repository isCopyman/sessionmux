"use client"

import { useMemo, useState } from "react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { SessionPicker } from "@/components/rooms/session-picker"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { useOpenRoom } from "@/lib/open-room"
import { defaultRoomTitle, roomMemberCandidates } from "@/lib/room-create"
import { useRoomMembership } from "@/lib/room-membership"
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
  folderScopeId = null,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** When set, initiator candidates are only Sessions in this Path. */
  folderScopeId?: number | null
}) {
  const t = useTranslations("Room")
  const conversations = useAppWorkspaceStore((state) => state.conversations)
  const folders = useAppWorkspaceStore((state) => state.folders)
  const activeWorkbenchId = useTabStore((state) => state.activeWorkbenchId)
  const activeConversationId = useTabStore((state) => {
    const tab = state.rawTabs.find((item) => item.id === state.activeTabId)
    if (tab == null || tab.kind !== "conversation") return null
    return tab.conversationId
  })
  const openRoom = useOpenRoom()
  const { createRoomWith } = useRoomMembership()
  const [query, setQuery] = useState("")
  const [selectedId, setSelectedId] = useState<number | null>(() => {
    if (activeConversationId == null) return null
    if (folderScopeId == null) return activeConversationId
    const active = conversations.find(
      (conversation) => conversation.id === activeConversationId
    )
    if (active == null || active.archived_at != null) return null
    if (active.folder_id !== folderScopeId) return null
    return activeConversationId
  })
  const [titleDraft, setTitleDraft] = useState<string | null>(null)
  const [pending, setPending] = useState(false)

  const scopeFolder = useMemo(() => {
    if (folderScopeId == null) return undefined
    return folders.find((folder) => folder.id === folderScopeId)
  }, [folderScopeId, folders])
  const scopeLabel =
    scopeFolder != null ? (scopeFolder.alias ?? scopeFolder.name) : null

  const candidates = useMemo(() => {
    const live = roomMemberCandidates(conversations, query)
    if (folderScopeId == null) return live
    return live.filter(
      (conversation) => conversation.folder_id === folderScopeId
    )
  }, [conversations, folderScopeId, query])
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
      const created = await createRoomWith({
        workbenchId: activeWorkbenchId,
        title: (titleDraft ?? "").trim() || autoTitle,
        memberConversationIds: [initiator.id],
        createdByConversationId: initiator.id,
      })
      if (created == null) return
      handleOpenChange(false)
      await openRoom(created)
    } finally {
      setPending(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {t("createTitle")}
            {folderScopeId != null ? (
              <span className="rounded-full bg-muted px-1.5 py-px text-[10px] font-normal text-muted-foreground">
                {scopeLabel != null && scopeLabel !== ""
                  ? t("createFolderScope", { name: scopeLabel })
                  : t("createFolderScopeGeneric")}
              </span>
            ) : null}
          </DialogTitle>
        </DialogHeader>
        <Input
          value={titleDraft ?? autoTitle}
          onChange={(event) => setTitleDraft(event.target.value)}
          placeholder={t("createNamePlaceholder")}
          aria-label={t("createNamePlaceholder")}
        />
        <SessionPicker
          mode="single"
          candidates={candidates}
          value={selectedId}
          onChange={setSelectedId}
          query={query}
          onQueryChange={setQuery}
          emptyLabel={t("createNoCandidates")}
        />
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
