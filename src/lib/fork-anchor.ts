import type { AgentType } from "./types"

/**
 * Backend `AcpError::ForkAnchorRejected` — the CLI rejected
 * `--resume-session-at` with a deterministic, non-retryable
 * `Resume rejected by --resume-drops-turn:` prefix. Re-sending the same
 * anchor always fails; callers must NOT re-queue.
 */
export class ForkAnchorRejectedError extends Error {
  constructor(message?: string) {
    super(message ?? "this position cannot be forked")
    this.name = "ForkAnchorRejectedError"
  }
}

/** Substring of the CLI / adapter rejection. Matching a prefix keeps
 *  recognition working if the backend wraps it in a Display string. */
export const FORK_ANCHOR_REJECTED_MARKER =
  "Resume rejected by --resume-drops-turn:"

/** Stable code from the web `AppErrorCode` body / `AcpError::code()`.
 *  Package G lands the backend variant; this is the name we recognize. */
export const FORK_ANCHOR_REJECTED_CODE = "fork_anchor_rejected"

export function nonemptyProviderAnchor(value?: string | null): string | null {
  if (typeof value !== "string") return null
  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : null
}

/** Last non-empty `provider_anchor` in a (possibly merged) turn group. */
export function lastProviderAnchor(
  turns: Array<{ provider_anchor?: string | null }> | undefined
): string | null {
  if (!turns || turns.length === 0) return null
  for (let i = turns.length - 1; i >= 0; i--) {
    const anchor = nonemptyProviderAnchor(turns[i]?.provider_anchor)
    if (anchor) return anchor
  }
  return null
}

/** Session-level + per-turn gate. Both required — a missing anchor would
 *  hit the non-retryable resume-drops-turn rejection. */
export function canShowForkAtMessage(
  agentType: AgentType | null | undefined,
  providerAnchor?: string | null
): boolean {
  return (
    agentType === "claude_code" &&
    nonemptyProviderAnchor(providerAnchor) != null
  )
}

/**
 * Wire args for `acp_fork`. `anchor` is omitted when empty so a head fork
 * payload stays `{connectionId, conversationId, folderId}` — the hard
 * regression line against today's invoke body.
 */
export function buildAcpForkArgs(input: {
  connectionId: string
  conversationId?: number | null
  folderId?: number | null
  anchor?: string | null
}): Record<string, unknown> {
  const args: Record<string, unknown> = {
    connectionId: input.connectionId,
    conversationId: input.conversationId ?? null,
    folderId: input.folderId ?? null,
  }
  const anchor = nonemptyProviderAnchor(input.anchor)
  if (anchor) args.anchor = anchor
  return args
}

export function isForkAnchorRejection(err: unknown): boolean {
  if (err instanceof ForkAnchorRejectedError) return true
  if (typeof err === "string") {
    return (
      err.includes(FORK_ANCHOR_REJECTED_MARKER) ||
      err.includes(FORK_ANCHOR_REJECTED_CODE)
    )
  }
  if (err && typeof err === "object") {
    if ((err as { code?: unknown }).code === FORK_ANCHOR_REJECTED_CODE) {
      return true
    }
    const message = (err as { message?: unknown }).message
    if (typeof message === "string") {
      return (
        message.includes(FORK_ANCHOR_REJECTED_MARKER) ||
        message.includes(FORK_ANCHOR_REJECTED_CODE)
      )
    }
  }
  return false
}
