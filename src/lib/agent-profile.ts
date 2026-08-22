import {
  claudeProfileList,
  conversationGetClaudeProfile,
  conversationSetClaudeProfile,
  openSettingsWindow,
} from "@/lib/api"
import type { AgentType } from "@/lib/types"
import {
  CODEG_CLAUDE_PROFILE_ENV_KEY,
  type ConversationAgentProfileResult,
} from "@/lib/types"

/**
 * Harness-agnostic projection of a launch profile.
 *
 * Provider-specific editing fields intentionally stay behind the adapter. The
 * Session launcher only needs a stable id, a human label and a short hint.
 */
export interface AgentLaunchProfileInfo {
  id: string
  label: string
  description: string | null
}

export interface AgentProfileAdapter {
  agentType: AgentType
  /** Agent-setting env key used for this Harness's default profile. */
  defaultProfileEnvKey: string | null
  list: () => Promise<AgentLaunchProfileInfo[]>
  getConversationProfile: (
    conversationId: number
  ) => Promise<ConversationAgentProfileResult>
  setConversationProfile: (
    conversationId: number,
    profileId: string | null
  ) => Promise<ConversationAgentProfileResult>
  manage: () => Promise<void>
}

const claudeProfileAdapter: AgentProfileAdapter = {
  agentType: "claude_code",
  defaultProfileEnvKey: CODEG_CLAUDE_PROFILE_ENV_KEY,
  list: async () =>
    (await claudeProfileList()).map((profile) => ({
      id: profile.id,
      label: profile.label,
      description:
        profile.kind === "configDir"
          ? profile.configDir?.trim() || null
          : profile.kind === "managed"
            ? profile.baseUrl?.trim() || null
            : null,
    })),
  getConversationProfile: (conversationId) =>
    conversationGetClaudeProfile(conversationId),
  setConversationProfile: (conversationId, profileId) =>
    conversationSetClaudeProfile(conversationId, profileId),
  manage: () => openSettingsWindow("agents", { agentType: "claude_code" }),
}

/**
 * Capability registry for launch profiles. Claude is the first implementation;
 * adding Codex/Grok later is an adapter registration, not another selector or
 * another Session persistence flow.
 */
export function getAgentProfileAdapter(
  agentType: AgentType
): AgentProfileAdapter | null {
  switch (agentType) {
    case "claude_code":
      return claudeProfileAdapter
    default:
      return null
  }
}

export function agentSupportsProfiles(agentType: AgentType): boolean {
  return getAgentProfileAdapter(agentType) != null
}
