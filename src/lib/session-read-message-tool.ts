const READ_MESSAGE_SUFFIX_RE = /[^a-z0-9]read_message$/

export function isSessionReadMessageToolName(toolName: string): boolean {
  const name = toolName.toLowerCase().trim()
  return name === "read_message" || READ_MESSAGE_SUFFIX_RE.test(name)
}

export interface SessionReadLetter {
  eventId: string
  fromSessionId: number | null
  fromTitle: string | null
  title: string
  body: string
  expectsReply: boolean
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  return value as Record<string, unknown>
}

function parseObject(raw: unknown): SessionReadLetter | null {
  const record = asRecord(raw)
  if (!record) return null
  const nested = asRecord(record.structuredContent) ?? record
  const eventId =
    (typeof nested.event_id === "string" && nested.event_id) ||
    (typeof nested.eventId === "string" && nested.eventId) ||
    ""
  const body =
    (typeof nested.body === "string" && nested.body) ||
    (typeof nested.content === "string" && nested.content) ||
    ""
  if (!eventId && !body) return null
  const title =
    (typeof nested.title === "string" && nested.title.trim()) ||
    (typeof nested.subject === "string" && nested.subject.trim()) ||
    ""
  const fromSessionId =
    typeof nested.from_session_id === "number"
      ? nested.from_session_id
      : typeof nested.fromSessionId === "number"
        ? nested.fromSessionId
        : null
  const fromTitle =
    typeof nested.from_title === "string"
      ? nested.from_title
      : typeof nested.fromTitle === "string"
        ? nested.fromTitle
        : null
  return {
    eventId,
    fromSessionId,
    fromTitle,
    title,
    body,
    expectsReply: nested.expects_reply === true || nested.expectsReply === true,
  }
}

export function parseSessionReadMessageOutput(
  output: string | null
): SessionReadLetter | null {
  if (!output) return null
  const trimmed = output.trim()
  if (trimmed.startsWith("{")) {
    try {
      const parsed: unknown = JSON.parse(trimmed)
      const fromStructured = parseObject(parsed)
      if (fromStructured) return fromStructured
      const record = asRecord(parsed)
      const content = record?.content
      if (Array.isArray(content)) {
        const text = content
          .map((item) => {
            if (!item || typeof item !== "object" || Array.isArray(item)) {
              return ""
            }
            const piece = item as Record<string, unknown>
            return typeof piece.text === "string" ? piece.text : ""
          })
          .filter(Boolean)
          .join("\n")
        if (text) return parseSessionReadMessageOutput(text)
      }
      return null
    } catch {
      return null
    }
  }
  const titleMatch = trimmed.match(/^Title:\s*(.+)$/m)
  const bodyIndex = trimmed.indexOf("--- message ---")
  const eventMatch = trimmed.match(
    /[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/
  )
  if (bodyIndex < 0 && !titleMatch) return null
  return {
    eventId: eventMatch?.[0] ?? "",
    fromSessionId: null,
    fromTitle: null,
    title: titleMatch?.[1]?.trim() ?? "",
    body: bodyIndex >= 0 ? trimmed.slice(bodyIndex + 15).trim() : trimmed,
    expectsReply: /expects a reply/i.test(trimmed),
  }
}
