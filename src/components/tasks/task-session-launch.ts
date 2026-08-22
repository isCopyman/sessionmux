import { persistPendingClaudeProfile } from "@/components/chat/apply-pending-claude-profile"
import {
  acpConnect,
  createConversation,
  workTaskAssignSession,
} from "@/lib/api"
import {
  CODEG_CLAUDE_PROFILE_CONFIG_KEY,
  type AgentType,
  type WorkTask,
  type WorkTaskConfig,
} from "@/lib/types"

/**
 * UI orchestration for the same product operation exposed to Agents as
 * `session.create` followed by `task.assign`: create one persistent Session,
 * then bind the neutral task to its stable id. This is not a distinct
 * Worktree Session type or a task-owned runtime.
 *
 * Identity is allocated before ACP starts. If launch or assignment fails, the
 * Session is intentionally preserved: it is a real user-visible Session that
 * can be repaired or assigned later, never a hidden provisional task runtime.
 */
export async function createTaskSessionAndAssign(options: {
  task: WorkTask
  folderPath: string
  config: WorkTaskConfig
}): Promise<number> {
  const { task, folderPath, config } = options
  if (config.agent_type == null) {
    throw new Error("A Harness is required to create a Session")
  }
  const agentType = config.agent_type as AgentType
  const conversationId = await createConversation(
    task.folder_id,
    agentType,
    task.title
  )
  const launchValues = { ...(config.config_values ?? {}) }
  const profileId = launchValues[CODEG_CLAUDE_PROFILE_CONFIG_KEY] ?? null
  delete launchValues[CODEG_CLAUDE_PROFILE_CONFIG_KEY]
  if (agentType === "claude_code") {
    await persistPendingClaudeProfile({
      conversationId,
      pendingProfileId: profileId,
    })
  }
  await acpConnect(
    agentType,
    folderPath,
    undefined,
    config.mode_id,
    launchValues,
    conversationId,
    true
  )
  await workTaskAssignSession(task.id, conversationId)
  return conversationId
}
