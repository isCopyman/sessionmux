import type { AgentType, ConnectionStatus } from "@/lib/types"

export type SessionLiveActivity = "busy" | "attention" | null

export interface WorkbenchActivityTab {
  workbenchId: number
  conversationId: number | null
}

export interface WorkbenchConnectionSnapshot {
  status: ConnectionStatus
}

export interface WorkbenchActivity {
  byConversationId: Map<number, Exclude<SessionLiveActivity, null>>
  busyCountByWorkbenchId: Map<number, number>
}

export function sessionActivityFromStatus(
  status: ConnectionStatus | null | undefined
): SessionLiveActivity {
  if (status === "prompting") return "busy"
  if (status === "error") return "attention"
  return null
}

/**
 * ACP connections are keyed by the tab id. Live tab ids win (including
 * `new-…` drafts that later bind a conversation). Persisted Session tabs
 * use `conv-{folderId}-{agentType}-{conversationId}` — the same format as
 * `makeConversationTabId` in the tab store — so inactive Workbench
 * snapshots can still find a keep-alive connection after a switch.
 */
export function connectionKeysForSession(session: {
  conversationId: number | null
  liveTabId?: string
  folderId: number
  agentType: AgentType
}): string[] {
  if (session.conversationId == null) return []
  const keys: string[] = []
  if (session.liveTabId) keys.push(session.liveTabId)
  const canonical = `conv-${session.folderId}-${session.agentType}-${session.conversationId}`
  if (!keys.includes(canonical)) keys.push(canonical)
  return keys
}

export function deriveWorkbenchActivity(
  tabs: ReadonlyArray<WorkbenchActivityTab>,
  connectionsByConversationId: ReadonlyMap<number, WorkbenchConnectionSnapshot>
): WorkbenchActivity {
  const byConversationId = new Map<number, Exclude<SessionLiveActivity, null>>()

  for (const tab of tabs) {
    if (tab.conversationId == null) continue
    if (byConversationId.has(tab.conversationId)) continue
    const activity = sessionActivityFromStatus(
      connectionsByConversationId.get(tab.conversationId)?.status
    )
    if (activity) byConversationId.set(tab.conversationId, activity)
  }

  const busyCountByWorkbenchId = new Map<number, number>()
  const counted = new Map<number, Set<number>>()

  for (const tab of tabs) {
    if (tab.conversationId == null) continue
    if (byConversationId.get(tab.conversationId) !== "busy") continue
    let seen = counted.get(tab.workbenchId)
    if (!seen) {
      seen = new Set()
      counted.set(tab.workbenchId, seen)
    }
    if (seen.has(tab.conversationId)) continue
    seen.add(tab.conversationId)
    busyCountByWorkbenchId.set(
      tab.workbenchId,
      (busyCountByWorkbenchId.get(tab.workbenchId) ?? 0) + 1
    )
  }

  return { byConversationId, busyCountByWorkbenchId }
}
