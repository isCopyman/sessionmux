import {
  assignConversationsToCollection,
  deleteConversation,
  updateConversationArchive,
} from "@/lib/api"
import type { AgentType, DbConversationSummary } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useOrganizationRevisionStore } from "@/stores/organization-revision-store"

export async function archiveSessions(
  conversations: readonly DbConversationSummary[],
  archived = true
): Promise<void> {
  await Promise.all(
    conversations.map((conversation) =>
      updateConversationArchive(conversation.id, archived)
    )
  )
  const now = new Date().toISOString()
  const { applyConversationUpsert } = useAppWorkspaceStore.getState()
  for (const conversation of conversations) {
    applyConversationUpsert({
      ...conversation,
      archived_at: archived ? now : null,
    })
  }
}

export async function deleteSessions(
  conversations: readonly DbConversationSummary[],
  closeConversationTab: (
    folderId: number,
    conversationId: number,
    agentType: AgentType
  ) => void
): Promise<void> {
  await Promise.all(
    conversations.map((conversation) => deleteConversation(conversation.id))
  )
  const { applyConversationRemove } = useAppWorkspaceStore.getState()
  for (const conversation of conversations) {
    closeConversationTab(
      conversation.folder_id,
      conversation.id,
      conversation.agent_type
    )
    applyConversationRemove(conversation.id)
  }
}

export async function moveSessionsToCollection(
  conversationIds: readonly number[],
  collectionId: number | null
) {
  const refs = await assignConversationsToCollection(
    [...conversationIds],
    collectionId
  )
  useOrganizationRevisionStore.getState().invalidate()
  return refs
}
