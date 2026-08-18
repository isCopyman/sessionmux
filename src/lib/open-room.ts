import { useCallback } from "react"

import { useTabActions, useTabStore } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useRoomCatalogStore } from "@/stores/room-catalog-store"

export interface OpenableRoom {
  id: string
  title: string
  workbenchId: number
  createdByConversationId: number
  collectionId?: number | null
  rootFolderId?: number | null
}

export function useOpenRoom() {
  const { openRoomTab, switchWorkbench } = useTabActions()
  const { openConversations } = useWorkbenchRoute()
  const activeWorkbenchId = useTabStore((state) => state.activeWorkbenchId)

  return useCallback(
    async (room: OpenableRoom) => {
      if (room.workbenchId !== activeWorkbenchId) {
        await switchWorkbench(room.workbenchId)
      }
      openConversations()
      const conversations = useAppWorkspaceStore.getState().conversations
      const folders = useAppWorkspaceStore.getState().folders
      const creator = conversations.find(
        (conversation) => conversation.id === room.createdByConversationId
      )
      openRoomTab({
        roomId: room.id,
        title: room.title,
        folderId:
          room.rootFolderId ??
          creator?.folder_id ??
          folders[0]?.id ??
          1,
        agentType: creator?.agent_type ?? "claude_code",
      })
      void useRoomCatalogStore.getState().refresh()
    },
    [activeWorkbenchId, openConversations, openRoomTab, switchWorkbench]
  )
}
