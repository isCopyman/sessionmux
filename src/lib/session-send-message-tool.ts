/** Host-prefixed or bare `send_message` (Claude `mcp__server__send_message`). */
const SEND_MESSAGE_SUFFIX_RE = /[^a-z0-9]send_message$/

export function isSessionSendMessageToolName(toolName: string): boolean {
  const name = toolName.toLowerCase().trim()
  return name === "send_message" || SEND_MESSAGE_SUFFIX_RE.test(name)
}

export function parseSessionSendMessageInput(input: string | null): {
  targetSessionIds: number[]
  title: string
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
    const title =
      typeof record.title === "string"
        ? record.title.trim()
        : typeof record.subject === "string"
          ? record.subject.trim()
          : ""
    if (targetSessionIds.length === 0 || !content) return null
    return { targetSessionIds, title, content }
  } catch {
    return null
  }
}

const EVENT_ID_RE = /[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/

export function parseSessionSendMessageEventId(
  output: string | null
): string | null {
  if (!output) return null
  const trimmed = output.trim()
  if (trimmed.startsWith("{")) {
    try {
      const parsed: unknown = JSON.parse(trimmed)
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        const record = parsed as Record<string, unknown>
        const eventId = record.event_id ?? record.eventId
        if (typeof eventId === "string" && EVENT_ID_RE.test(eventId)) {
          return eventId
        }
      }
    } catch {
      // Fall through to the prose form from the MCP companion.
    }
  }
  const match = trimmed.match(
    new RegExp(`event (${EVENT_ID_RE.source})`, "i")
  )
  return match?.[1] ?? null
}
