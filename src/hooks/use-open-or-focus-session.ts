"use client"

import { useCallback } from "react"

import { useTabActions, useTabStore } from "@/contexts/tab-context"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import { openOrFocusSession } from "@/lib/open-or-focus-session"
import type { AgentType, DbConversationSummary } from "@/lib/types"

export function useOpenOrFocusSession() {
  const { openTab, switchWorkbench } = useTabActions()
  const { openConversations } = useWorkbenchRoute()
  const currentWorkbenchId =
    useTabStore((state) => state.activeWorkbenchId) ?? 1

  return useCallback(
    (
      conversation: Pick<
        DbConversationSummary,
        "id" | "folder_id" | "agent_type" | "title"
      > & { agent_type: AgentType },
      options?: { split?: "right" | "down" }
    ) =>
      openOrFocusSession({
        conversation,
        currentWorkbenchId,
        switchWorkbench,
        openTab,
        openConversations,
        split: options?.split,
      }),
    [currentWorkbenchId, openConversations, openTab, switchWorkbench]
  )
}
