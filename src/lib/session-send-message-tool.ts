/** Host-prefixed or bare `send_message` (Claude `mcp__server__send_message`). */
const SEND_MESSAGE_SUFFIX_RE = /[^a-z0-9]send_message$/

export function isSessionSendMessageToolName(toolName: string): boolean {
  const name = toolName.toLowerCase().trim()
  return name === "send_message" || SEND_MESSAGE_SUFFIX_RE.test(name)
}

export function parseSessionSendMessageInput(input: string | null): {
  targetSessionIds: number[]
  content: string
} | null {
  if (!input) return null
  const trimmed = input.trim()
  if (!trimmed.startsWith("{")) return null
  try {
    const parsed: unknown = JSON.parse(trimmed)
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return null
    }
    const record = parsed as Record<string, unknown>
    const rawIds = record.target_session_ids
    if (!Array.isArray(rawIds)) return null
    const targetSessionIds = rawIds.filter(
      (id): id is number =>
        typeof id === "number" && Number.isInteger(id) && id > 0
    )
    const content =
      typeof record.content === "string" ? record.content.trim() : ""
    if (targetSessionIds.length === 0 || !content) return null
    return { targetSessionIds, content }
  } catch {
    return null
  }
}
