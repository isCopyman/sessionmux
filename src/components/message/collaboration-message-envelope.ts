export interface CollaborationMessageEnvelope {
  version: 1
  eventId: string
  deliveryId: string
  sourceConversationId: number
  sourceTitle: string | null
  sourceAgentType: string
  sourceFolderPath: string | null
  letterTitle: string | null
  kind: "system_notify" | "letter"
  expectsReply: boolean
  replyToEventId: string | null
  body: string
}

const PREFIX = "<<<CODEG_SESSION_MESSAGE_V1:"
const END_PREFIX = "<<<END_CODEG_SESSION_MESSAGE_V1:"
const MESSAGE_SEPARATOR = "--- message ---"

function optionalString(value: unknown): value is string | null {
  return value == null || typeof value === "string"
}

/**
 * Parse only the complete V1 envelope emitted by the Codeg backend. Any
 * partial, malformed, or future envelope deliberately falls back to ordinary
 * transcript text so users never lose access to the raw Harness record.
 */
export function parseCollaborationMessageEnvelope(
  text: string
): CollaborationMessageEnvelope | null {
  const lines = text.split("\n")
  if (lines.length < 5) return null
  const first = lines[0]
  if (!first.startsWith(PREFIX) || !first.endsWith(">>>")) return null
  const eventId = first.slice(PREFIX.length, -3)
  if (!/^[0-9a-fA-F-]{36}$/.test(eventId)) return null
  if (lines[lines.length - 1] !== `${END_PREFIX}${eventId}>>>`) return null
  const separatorIndex = lines.indexOf(MESSAGE_SEPARATOR, 2)
  if (separatorIndex < 2) return null

  let metadata: Record<string, unknown>
  try {
    const parsed: unknown = JSON.parse(lines[1])
    if (parsed == null || typeof parsed !== "object" || Array.isArray(parsed)) {
      return null
    }
    metadata = parsed as Record<string, unknown>
  } catch {
    return null
  }
  if (
    metadata.version !== 1 ||
    metadata.eventId !== eventId ||
    typeof metadata.deliveryId !== "string" ||
    typeof metadata.sourceConversationId !== "number" ||
    !Number.isInteger(metadata.sourceConversationId) ||
    !optionalString(metadata.sourceTitle) ||
    typeof metadata.sourceAgentType !== "string" ||
    !optionalString(metadata.sourceFolderPath) ||
    typeof metadata.expectsReply !== "boolean" ||
    !optionalString(metadata.replyToEventId)
  ) {
    return null
  }
  const letterTitle = optionalString(metadata.letterTitle)
    ? metadata.letterTitle
    : null
  const body = lines.slice(separatorIndex + 1, -1).join("\n")
  const kind =
    metadata.kind === "letter"
      ? "letter"
      : metadata.kind === "system_notify"
        ? "system_notify"
        : body.trim().length > 0
          ? "letter"
          : "system_notify"

  return {
    version: 1,
    eventId,
    deliveryId: metadata.deliveryId,
    sourceConversationId: metadata.sourceConversationId,
    sourceTitle: metadata.sourceTitle,
    sourceAgentType: metadata.sourceAgentType,
    sourceFolderPath: metadata.sourceFolderPath,
    letterTitle,
    kind,
    expectsReply: metadata.expectsReply,
    replyToEventId: metadata.replyToEventId,
    body,
  }
}

/**
 * Remove only complete, backend-authored envelopes whose immutable event id is
 * already represented by a collaboration timeline card. Malformed, unknown,
 * or unprojected text stays visible verbatim so this adapter cannot hide
 * Harness history when the projection is incomplete.
 */
/** Find complete V1 envelopes in transcript text, in order. */
export function extractCollaborationEnvelopes(
  text: string
): CollaborationMessageEnvelope[] {
  if (!text.includes(PREFIX)) return []
  const lines = text.split("\n")
  const found: CollaborationMessageEnvelope[] = []
  let index = 0
  while (index < lines.length) {
    const first = lines[index]
    const eventId =
      first.startsWith(PREFIX) && first.endsWith(">>>")
        ? first.slice(PREFIX.length, -3)
        : null
    if (eventId) {
      const endMarker = `${END_PREFIX}${eventId}>>>`
      const endIndex = lines.indexOf(endMarker, index + 1)
      if (endIndex >= 0) {
        const candidate = lines.slice(index, endIndex + 1).join("\n")
        const parsed = parseCollaborationMessageEnvelope(candidate)
        if (parsed) {
          found.push(parsed)
          index = endIndex + 1
          continue
        }
      }
    }
    index += 1
  }
  return found
}

export function stripProjectedCollaborationEnvelopes(
  text: string,
  projectedEventIds: ReadonlySet<string>
): string {
  if (projectedEventIds.size === 0 || !text.includes(PREFIX)) return text
  const lines = text.split("\n")
  const retained: string[] = []
  let index = 0

  while (index < lines.length) {
    const first = lines[index]
    const eventId =
      first.startsWith(PREFIX) && first.endsWith(">>>")
        ? first.slice(PREFIX.length, -3)
        : null
    if (eventId && projectedEventIds.has(eventId)) {
      const endMarker = `${END_PREFIX}${eventId}>>>`
      const endIndex = lines.indexOf(endMarker, index + 1)
      if (endIndex >= 0) {
        const candidate = lines.slice(index, endIndex + 1).join("\n")
        const parsed = parseCollaborationMessageEnvelope(candidate)
        if (parsed?.eventId === eventId) {
          index = endIndex + 1
          continue
        }
      }
    }
    retained.push(first)
    index += 1
  }

  return retained.join("\n")
}
