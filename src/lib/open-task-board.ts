"use client"

import { useCallback } from "react"
import { useTranslations } from "next-intl"

import { useTabActions } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import {
  GLOBAL_TASK_BOARD_SCOPE,
  taskBoardProjectId,
  type TaskBoardScope,
} from "@/lib/task-board-scope"
import type { AgentType } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"

/** opened_tab.agent_type is still non-null although a Board has no Harness. */
export const BOARD_TAB_PLACEHOLDER_AGENT: AgentType = "claude_code"

export function useOpenTaskBoard() {
  const t = useTranslations("Tasks")
  const { openBoardTab } = useTabActions()
  const { openConversations } = useWorkbenchRoute()

  return useCallback(
    (scope: TaskBoardScope = GLOBAL_TASK_BOARD_SCOPE): boolean => {
      const { folders, allFolders, activeFolderId } =
        useAppWorkspaceStore.getState()
      const projects = folders.filter(
        (folder) => folder.parent_id == null && folder.kind === "regular"
      )
      const projectId = taskBoardProjectId(scope)
      const active = allFolders.find((folder) => folder.id === activeFolderId)
      const activeRootId = active?.parent_id ?? active?.id ?? null
      const anchorId =
        projectId ??
        (activeRootId != null &&
        projects.some((project) => project.id === activeRootId)
          ? activeRootId
          : (projects[0]?.id ?? null))
      if (anchorId == null) return false

      const project = allFolders.find((folder) => folder.id === projectId)
      openConversations()
      openBoardTab({
        scope,
        title:
          projectId == null
            ? t("title")
            : `${project?.alias ?? project?.name ?? `#${projectId}`} · ${t("title")}`,
        folderId: anchorId,
        agentType: BOARD_TAB_PLACEHOLDER_AGENT,
      })
      return true
    },
    [openBoardTab, openConversations, t]
  )
}
