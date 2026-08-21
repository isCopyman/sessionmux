import { conversationSetClaudeProfile } from "@/lib/api"

/**
 * Persist the launch profile that already succeeded on the draft connection.
 * The launch itself uses an explicit connect-time pin; creating the conversation
 * only records that fact for later Resume and must not restart the process again.
 */
export async function persistPendingClaudeProfile(options: {
  conversationId: number
  pendingProfileId: string | null
}): Promise<boolean> {
  if (options.pendingProfileId == null) return false
  await conversationSetClaudeProfile(
    options.conversationId,
    options.pendingProfileId
  )
  return true
}
