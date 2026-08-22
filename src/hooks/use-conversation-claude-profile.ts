"use client"

import { useEffect, useState } from "react"

import { conversationGetClaudeProfile } from "@/lib/api"

/**
 * Resolve the launch profile bound to a managed Claude Session. Native
 * transcripts expose the model per turn but do not record Codeg's launch
 * profile, so the Session timeline presents the stable conversation binding.
 * Room posts carry their own immutable profile snapshot instead.
 */
export function useConversationClaudeProfile(
  conversationId: number,
  enabled: boolean
): string | null {
  const [resolved, setResolved] = useState<{
    conversationId: number
    profile: string | null
  } | null>(null)

  useEffect(() => {
    if (!enabled) return
    let cancelled = false
    void conversationGetClaudeProfile(conversationId)
      .then((result) => {
        if (!cancelled) {
          setResolved({
            conversationId,
            profile: result.profileId?.trim() || null,
          })
        }
      })
      .catch(() => {
        if (!cancelled) setResolved({ conversationId, profile: null })
      })
    return () => {
      cancelled = true
    }
  }, [conversationId, enabled])

  return enabled && resolved?.conversationId === conversationId
    ? resolved.profile
    : null
}
