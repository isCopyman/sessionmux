import { useCallback } from "react"

import { useTabActions, useTabStore } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import type { AgentType } from "@/lib/types"
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

/** Schema placeholder: opened_tab.agent_type is still NOT NULL. Room tabs
 * do not run an agent; hydrate ignores this for identity. */
export const ROOM_TAB_PLACEHOLDER_AGENT: AgentType = "claude_code"

export function roomTabFolderId(
  room: Pick<OpenableRoom, "rootFolderId">,
  folders: { id: number }[]
): number {
  return room.rootFolderId ?? folders[0]?.id ?? 1
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
      const folders = useAppWorkspaceStore.getState().folders
      openRoomTab({
        roomId: room.id,
        title: room.title,
        folderId: roomTabFolderId(room, folders),
        agentType: ROOM_TAB_PLACEHOLDER_AGENT,
      })
      void useRoomCatalogStore.getState().refresh()
    },
    [activeWorkbenchId, openConversations, openRoomTab, switchWorkbench]
  )
}
