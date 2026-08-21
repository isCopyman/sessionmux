import { conversationSetClaudeProfile } from "@/lib/api"

/**
 * Persist the composer's pending Claude launch profile onto a newly created
 * conversation, then respawn so the first prompt cannot run on the process
 * that auto-connected before this row existed.
 *
 * Call after `createConversation` / `createChatConversation` and before
 * `lifecycleSend`. A no-op when there is no pending choice (the picker was
 * never touched). Returns whether a write+respawn ran.
 *
 * Ordering is the contract: the profile write is awaited, then disconnect,
 * then connect with this conversation id. `connect()` must not be called
 * against a live same-agent/cwd connection — that path no-ops and would
 * leave the first prompt on the unbound spawn.
 */
export async function applyPendingClaudeProfileAndRespawn(options: {
  conversationId: number
  pendingProfileId: string | null
  disconnect: () => Promise<unknown>
  connect: (conversationId: number) => Promise<unknown>
}): Promise<boolean> {
  if (options.pendingProfileId == null) return false
  await conversationSetClaudeProfile(
    options.conversationId,
    options.pendingProfileId
  )
  await options.disconnect()
  await options.connect(options.conversationId)
  return true
}
