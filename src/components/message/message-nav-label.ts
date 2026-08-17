import { extractCollaborationEnvelopes } from "./collaboration-message-envelope"

export interface MessageNavMailAttribution {
  conversationId: number
  title?: string | null
  agentType?: string | null
  eventIds: string[]
}

export interface MessageNavPreview {
  label: string
  sessionMail: MessageNavMailAttribution | null
}

const NAV_LABEL_MAX = 80

export function clipMessageNavLabel(text: string): string {
  const trimmed = text.replace(/\s+/g, " ").trim()
  if (!trimmed) return ""
  if (trimmed.length > NAV_LABEL_MAX) return `${trimmed.slice(0, 77)}...`
  return trimmed
}

export function resolveMessageNavPreview(input: {
  raw: string
  projectedText?: string | null
  sessionMail?: MessageNavMailAttribution | null
}): MessageNavPreview {
  const envelopes = extractCollaborationEnvelopes(input.raw)
  const envelopeMail =
    envelopes.length > 0
      ? {
          conversationId: envelopes[0].sourceConversationId,
          title: envelopes[0].sourceTitle,
          agentType: envelopes[0].sourceAgentType,
          eventIds: envelopes.map((envelope) => envelope.eventId),
        }
      : null
  const envelopeBody = envelopes
    .map((envelope) => envelope.body)
    .filter((body) => body.trim().length > 0)
    .join("\n\n")
  const label =
    clipMessageNavLabel(input.projectedText ?? "") ||
    clipMessageNavLabel(envelopeBody) ||
    clipMessageNavLabel(input.raw) ||
    "User message"
  return {
    label,
    sessionMail: input.sessionMail ?? envelopeMail,
  }
}
