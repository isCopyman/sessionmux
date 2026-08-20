import type { AgentType, DbConversationSummary } from "@/lib/types"
import { locateConversationHome } from "@/lib/workbench-session-tabs"

export type OpenConversationTabFn = (
  folderId: number,
  conversationId: number,
  agentType: AgentType,
  pin?: boolean,
  title?: string,
  options?: { split?: "right" | "down"; targetGroup?: string }
) => void

export type OpenOrFocusResult = "focused" | "switched" | "opened"

/**
 * Open-and-focus: if the session already has a workbench home, switch there
 * and focus the existing tab. Otherwise open a tab on the current workbench.
 * Does not delete leftover duplicate tabs from the old per-workbench dedupe.
 */
export async function openOrFocusSession(params: {
  conversation: Pick<
    DbConversationSummary,
    "id" | "folder_id" | "agent_type" | "title"
  >
  currentWorkbenchId: number
  switchWorkbench: (workbenchId: number) => Promise<void>
  openTab: OpenConversationTabFn
  openConversations?: () => void
  split?: "right" | "down"
}): Promise<OpenOrFocusResult> {
  const {
    conversation,
    currentWorkbenchId,
    switchWorkbench,
    openTab,
    openConversations,
    split,
  } = params

  const home = await locateConversationHome(conversation.id, currentWorkbenchId)

  if (home && home.workbench_id !== currentWorkbenchId) {
    await switchWorkbench(home.workbench_id)
    openConversations?.()
    openTab(
      conversation.folder_id,
      conversation.id,
      conversation.agent_type,
      true,
      conversation.title ?? undefined
    )
    return "switched"
  }

  openConversations?.()
  openTab(
    conversation.folder_id,
    conversation.id,
    conversation.agent_type,
    true,
    conversation.title ?? undefined,
    split ? { split } : undefined
  )
  return home ? "focused" : "opened"
}
