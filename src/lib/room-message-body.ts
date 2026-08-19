import {
  tokenizeReferenceLinks,
  unescapeReferenceLabel,
  unwrapReferenceDestination,
} from "@/lib/reference-link"
import { formatConversationTitle } from "@/lib/conversation-title"
import { parseCodegReferenceUri } from "@/components/chat/composer/reference-uri"

export type RoomMemberAlias = {
  conversationId: number
  title?: string | null
}

export type RoomBodyPart =
  | { type: "text"; value: string }
  | {
      type: "mention"
      kind: "session" | "all" | "human"
      conversationId?: number
      label: string
    }
  | {
      type: "reference"
      refType: "file" | "commit" | "other"
      label: string
      uri: string
    }

const SESSION_URI = /^codeg:\/\/session\/(\d+)$/i
const HUMAN_URI = /^codeg:\/\/(?:human|user)$/i
// Structured "wake everyone" token. The composer panel and @all button insert
// it as a reference badge (`[@all](codeg://all)`), so detection keys on the
// URI — the badge's visible label is localized and must not be load-bearing.
const ALL_URI = /^codeg:\/\/all(?![a-z0-9])/i
const ALL_AT = /^@(?:all|everyone|全体)$/i

export function mentionMarkdownForSession(
  label: string,
  conversationId: number
): string {
  return `[${label}](codeg://session/${conversationId})`
}

export function mentionAllFromText(text: string): boolean {
  return (
    // The URI shape is matched unanchored here (the token sits inside a
    // `[label](uri)` link); `codeg://allowed` must NOT match.
    /codeg:\/\/all(?![a-z0-9])/i.test(text) ||
    /@all\b/i.test(text) ||
    /@everyone\b/i.test(text) ||
    /@全体/.test(text)
  )
}

export function insertMentionToken(body: string, token: string): string {
  if (!token) return body
  if (body.includes(token)) return body
  if (!body.trim()) return token
  return /\s$/.test(body) ? `${body}${token}` : `${body} ${token}`
}

export function removeMentionToken(body: string, token: string): string {
  if (!token || !body.includes(token)) return body
  return body
    .split(token)
    .join("")
    .replace(/[ \t]{2,}/g, " ")
    .replace(/ +\n/g, "\n")
    .trimStart()
}

function memberLabel(
  member: RoomMemberAlias,
  untitled: (id: number) => string
): string {
  const title = formatConversationTitle(member.title).trim()
  return title || untitled(member.conversationId)
}

function compactName(name: string): string {
  return name.replace(/\s+/g, "")
}

function destinationUri(raw: string): string {
  return unwrapReferenceDestination(raw).trim()
}

function sessionIdFromUri(uri: string): number | null {
  const match = SESSION_URI.exec(uri)
  if (!match) return null
  const id = Number(match[1])
  return Number.isInteger(id) && id > 0 ? id : null
}

function aliasesForMember(
  member: RoomMemberAlias,
  untitled: (id: number) => string
): string[] {
  const label = memberLabel(member, untitled)
  const names = [label, compactName(label)].filter(Boolean)
  const unique: string[] = []
  for (const name of names) {
    const at = name.startsWith("@") ? name : `@${name}`
    if (!unique.some((item) => item.toLowerCase() === at.toLowerCase())) {
      unique.push(at)
    }
  }
  return unique
}

function matchAtToken(
  text: string,
  index: number,
  members: RoomMemberAlias[],
  untitled: (id: number) => string,
  allLabel: string,
  humanLabel: string
): { part: RoomBodyPart; length: number } | null {
  if (text[index] !== "@") return null
  const rest = text.slice(index)
  const allCandidates = ["@all", "@everyone", "@全体", allLabel].filter(Boolean)
  for (const token of allCandidates) {
    if (rest.toLowerCase().startsWith(token.toLowerCase())) {
      const after = rest[token.length]
      if (after && /[A-Za-z0-9_]/.test(after)) continue
      return {
        part: { type: "mention", kind: "all", label: allLabel },
        length: token.length,
      }
    }
  }
  const humanCandidates = [
    "@human",
    "@user",
    "@人类",
    "@人類",
    humanLabel,
  ].filter(Boolean)
  for (const token of humanCandidates) {
    if (rest.toLowerCase().startsWith(token.toLowerCase())) {
      const after = rest[token.length]
      if (after && /[A-Za-z0-9_]/.test(after)) continue
      return {
        part: { type: "mention", kind: "human", label: humanLabel },
        length: token.length,
      }
    }
  }
  let best: { length: number; part: RoomBodyPart } | null = null
  for (const member of members) {
    for (const token of aliasesForMember(member, untitled)) {
      if (!rest.toLowerCase().startsWith(token.toLowerCase())) continue
      const after = rest[token.length]
      if (after && /[A-Za-z0-9_]/.test(after)) continue
      if (!best || token.length > best.length) {
        best = {
          length: token.length,
          part: {
            type: "mention",
            kind: "session",
            conversationId: member.conversationId,
            label: `@${memberLabel(member, untitled)}`,
          },
        }
      }
    }
  }
  return best
}

function scanProse(
  text: string,
  members: RoomMemberAlias[],
  untitled: (id: number) => string,
  allLabel: string,
  humanLabel: string
): RoomBodyPart[] {
  const parts: RoomBodyPart[] = []
  let i = 0
  let textStart = 0
  const flush = (end: number) => {
    if (end > textStart) {
      parts.push({ type: "text", value: text.slice(textStart, end) })
    }
  }
  while (i < text.length) {
    const rawUri = text.slice(i).match(/^codeg:\/\/session\/(\d+)/i)
    if (rawUri) {
      const id = Number(rawUri[1])
      const member = members.find((item) => item.conversationId === id)
      flush(i)
      parts.push({
        type: "mention",
        kind: "session",
        conversationId: id,
        label: `@${member ? memberLabel(member, untitled) : untitled(id)}`,
      })
      i += rawUri[0].length
      textStart = i
      continue
    }
    if (/^codeg:\/\/(?:human|user)(?![a-z0-9])/i.test(text.slice(i))) {
      const raw = text.slice(i).match(/^codeg:\/\/(?:human|user)/i)
      flush(i)
      parts.push({ type: "mention", kind: "human", label: humanLabel })
      i += raw?.[0].length ?? 0
      textStart = i
      continue
    }
    if (/^codeg:\/\/all(?![a-z0-9])/i.test(text.slice(i))) {
      flush(i)
      parts.push({ type: "mention", kind: "all", label: allLabel })
      i += "codeg://all".length
      textStart = i
      continue
    }
    const at = matchAtToken(text, i, members, untitled, allLabel, humanLabel)
    if (at) {
      flush(i)
      parts.push(at.part)
      i += at.length
      textStart = i
      continue
    }
    i += 1
  }
  flush(text.length)
  return parts
}

export function sessionIdsFromAtAliases(
  text: string,
  members: RoomMemberAlias[],
  untitled: (id: number) => string
): number[] {
  const parts = scanProse(text, members, untitled, "@all", "@human")
  const ids = new Set<number>()
  for (const part of parts) {
    if (
      part.type === "mention" &&
      part.kind === "session" &&
      part.conversationId
    ) {
      ids.add(part.conversationId)
    }
  }
  return [...ids]
}

export function roomMessageBodyParts(input: {
  body: string
  members: RoomMemberAlias[]
  mentionConversationIds: number[]
  mentionHuman?: boolean
  allLabel: string
  humanLabel: string
  untitled: (id: number) => string
}): RoomBodyPart[] {
  const {
    body,
    members,
    mentionConversationIds,
    mentionHuman = false,
    allLabel,
    humanLabel,
    untitled,
  } = input
  const parts: RoomBodyPart[] = []
  for (const token of tokenizeReferenceLinks(body)) {
    if (token.type === "text") {
      parts.push(
        ...scanProse(token.value, members, untitled, allLabel, humanLabel)
      )
      continue
    }
    const uri = destinationUri(token.destination)
    const sessionId = sessionIdFromUri(uri)
    if (sessionId != null) {
      const member = members.find((item) => item.conversationId === sessionId)
      parts.push({
        type: "mention",
        kind: "session",
        conversationId: sessionId,
        label: `@${member ? memberLabel(member, untitled) : unescapeReferenceLabel(token.label)}`,
      })
      continue
    }
    const label = unescapeReferenceLabel(token.label)
    if (HUMAN_URI.test(uri) || ALL_URI.test(uri) || ALL_AT.test(label)) {
      const human = HUMAN_URI.test(uri)
      parts.push({
        type: "mention",
        kind: human ? "human" : "all",
        label: human ? humanLabel : allLabel,
      })
      continue
    }
    // File/commit badges from the `@` panel's file/commit tabs. Anything else
    // `parseCodegReferenceUri` recognizes (agent/skill/embedded — the room
    // composer never inserts those) stays raw text, same as an unrecognized
    // link.
    const parsed = parseCodegReferenceUri(uri, label)
    if (parsed && (parsed.refType === "file" || parsed.refType === "commit")) {
      parts.push({
        type: "reference",
        refType: parsed.refType,
        label: parsed.label,
        // The pre-parse uri (`parsed.uri` is nullable for uri-less kinds).
        uri,
      })
      continue
    }
    parts.push({ type: "text", value: token.raw })
  }

  const seenIds = new Set<number>()
  let sawAll = false
  let sawHuman = false
  for (const part of parts) {
    if (part.type !== "mention") continue
    if (part.kind === "all") sawAll = true
    if (part.kind === "human") sawHuman = true
    if (part.kind === "session" && part.conversationId) {
      seenIds.add(part.conversationId)
    }
  }

  const extras: RoomBodyPart[] = []
  if (!sawAll) {
    for (const id of mentionConversationIds) {
      if (seenIds.has(id)) continue
      const member = members.find((item) => item.conversationId === id)
      extras.push({
        type: "mention",
        kind: "session",
        conversationId: id,
        label: `@${member ? memberLabel(member, untitled) : untitled(id)}`,
      })
    }
  }
  if (mentionHuman && !sawHuman) {
    extras.push({ type: "mention", kind: "human", label: humanLabel })
  }

  if (extras.length === 0) return mergeText(parts)
  const spacer: RoomBodyPart[] = []
  const last = parts[parts.length - 1]
  if (last?.type === "text") {
    if (last.value.length > 0 && !/\s$/.test(last.value)) {
      spacer.push({ type: "text", value: " " })
    }
  } else if (last) {
    // A non-text part (a mention or a file/commit reference badge) sits right
    // before the appended extras — without a spacer the two inline chips
    // would run together with no gap.
    spacer.push({ type: "text", value: " " })
  }
  // `!last`: body empty except metadata — still show mentions, no spacer needed.
  return mergeText([...parts, ...spacer, ...extras])
}

function mergeText(parts: RoomBodyPart[]): RoomBodyPart[] {
  const out: RoomBodyPart[] = []
  for (const part of parts) {
    const prev = out[out.length - 1]
    if (part.type === "text" && prev?.type === "text") {
      prev.value += part.value
    } else {
      out.push({ ...part })
    }
  }
  // Non-text parts (mentions, file/commit references) are always kept; only a
  // genuinely empty text run is dropped.
  return out.filter((part) => part.type !== "text" || part.value.length > 0)
}
