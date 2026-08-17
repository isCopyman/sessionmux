import {
  listOpenedTabs,
  listWorkbenchTabs,
  saveOpenedTabs,
  saveWorkbenchTabs,
} from "@/lib/api"
import type { DbConversationSummary, OpenedTab } from "@/lib/types"

export const SIDEBAR_BULK_TAB_ORIGIN = "sidebar-bulk"
export const SESSION_CENTER_TAB_ORIGIN = "session-center"

export function conversationIdsInTabs(items: OpenedTab[]): Set<number> {
  const ids = new Set<number>()
  for (const item of items) {
    if (item.conversation_id != null) ids.add(item.conversation_id)
  }
  return ids
}

export function appendConversationTabs(
  existing: OpenedTab[],
  conversations: DbConversationSummary[]
): { items: OpenedTab[]; added: number; skipped: number } {
  const present = conversationIdsInTabs(existing)
  const toAdd = conversations.filter(
    (conversation) => !present.has(conversation.id)
  )
  const skipped = conversations.length - toAdd.length
  if (toAdd.length === 0) {
    return { items: existing, added: 0, skipped }
  }
  const nextPosition =
    existing.length === 0
      ? 0
      : Math.max(...existing.map((item) => item.position)) + 1
  return {
    items: [
      ...existing,
      ...toAdd.map((conversation, index) => ({
        id: 0,
        folder_id: conversation.folder_id,
        conversation_id: conversation.id,
        agent_type: conversation.agent_type,
        position: nextPosition + index,
        is_active: false,
        is_pinned: true,
      })),
    ],
    added: toAdd.length,
    skipped,
  }
}

async function listTabsForWorkbench(workbenchId: number) {
  return workbenchId === 1 ? listOpenedTabs() : listWorkbenchTabs(workbenchId)
}

async function saveTabsForWorkbench(
  workbenchId: number,
  items: OpenedTab[],
  expectedVersion: number,
  origin: string
) {
  return workbenchId === 1
    ? saveOpenedTabs(items, expectedVersion, origin)
    : saveWorkbenchTabs(workbenchId, items, expectedVersion, origin)
}

export async function appendConversationsToWorkbench(
  workbenchId: number,
  conversations: DbConversationSummary[],
  origin = SIDEBAR_BULK_TAB_ORIGIN
): Promise<{ added: number; skipped: number }> {
  const snapshot = await listTabsForWorkbench(workbenchId)
  let planned = appendConversationTabs(snapshot.items, conversations)
  if (planned.added === 0) {
    return { added: 0, skipped: planned.skipped }
  }

  let outcome = await saveTabsForWorkbench(
    workbenchId,
    planned.items,
    snapshot.version,
    origin
  )
  if (outcome.accepted) {
    return { added: planned.added, skipped: planned.skipped }
  }

  planned = appendConversationTabs(outcome.tabs, conversations)
  if (planned.added === 0) {
    return { added: 0, skipped: planned.skipped }
  }

  outcome = await saveTabsForWorkbench(
    workbenchId,
    planned.items,
    outcome.version,
    origin
  )
  if (!outcome.accepted) {
    throw new Error("Workbench changed concurrently; please retry")
  }
  return { added: planned.added, skipped: planned.skipped }
}
