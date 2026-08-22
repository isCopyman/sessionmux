import { getAgentProfileAdapter } from "@/lib/agent-profile"
import type { AgentType } from "@/lib/types"

/**
 * Persist the launch profile that already succeeded on a draft connection.
 * The selected Harness adapter owns validation and storage; callers do not
 * need a Claude/Codex/Grok branch.
 */
export async function persistPendingAgentProfile(options: {
  agentType: AgentType
  conversationId: number
  pendingProfileId: string | null
}): Promise<boolean> {
  if (options.pendingProfileId == null) return false
  const adapter = getAgentProfileAdapter(options.agentType)
  if (!adapter) return false
  await adapter.setConversationProfile(
    options.conversationId,
    options.pendingProfileId
  )
  return true
}
