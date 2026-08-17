import type { CollaborationDelivery } from "@/lib/types"
import { isAgentUnread } from "@/lib/mail-human-status"

export interface MailThread {
  rootEventId: string
  subject: string
  items: CollaborationDelivery[]
}

/** List-row facts for one thread, seen from `selfConversationId`'s side. */
export interface MailThreadSummary {
  /** Inbound letters this session's agent has not received yet. */
  unreadCount: number
  /** This session still owes a reply somewhere in the thread. */
  needsReply: boolean
  /** A peer still owes this session a reply. */
  awaitingReply: boolean
  failed: boolean
  latestAt: string
  /** Other participants, in first-appearance order. */
  peerIds: number[]
}

export function summarizeMailThread(
  thread: MailThread,
  selfConversationId: number
): MailThreadSummary {
  const summary: MailThreadSummary = {
    unreadCount: 0,
    needsReply: false,
    awaitingReply: false,
    failed: false,
    latestAt: thread.items[thread.items.length - 1]?.createdAt ?? "",
    peerIds: [],
  }
  for (const item of thread.items) {
    const outbound = item.source.conversationId === selfConversationId
    const peerId = outbound
      ? item.target.conversationId
      : item.source.conversationId
    if (!summary.peerIds.includes(peerId)) summary.peerIds.push(peerId)
    if (!outbound && isAgentUnread(item)) summary.unreadCount += 1
    if (item.obligationState === "awaiting_reply") {
      if (outbound) summary.awaitingReply = true
      else summary.needsReply = true
    }
    if (item.state === "failed") summary.failed = true
  }
  return summary
}

export function groupMailThreads(items: CollaborationDelivery[]): MailThread[] {
  const byEvent = new Map<string, CollaborationDelivery>()
  for (const item of items) {
    if (!byEvent.has(item.eventId)) byEvent.set(item.eventId, item)
  }

  const rootOf = (item: CollaborationDelivery): string => {
    let current = item
    const seen = new Set<string>()
    while (
      current.replyToEventId &&
      byEvent.has(current.replyToEventId) &&
      !seen.has(current.eventId)
    ) {
      seen.add(current.eventId)
      current = byEvent.get(current.replyToEventId) ?? current
    }
    return current.eventId
  }

  const threads = new Map<string, CollaborationDelivery[]>()
  for (const item of items) {
    const root = rootOf(item)
    const list = threads.get(root) ?? []
    list.push(item)
    threads.set(root, list)
  }

  return [...threads.entries()]
    .map(([rootEventId, threadItems]) => {
      const sorted = [...threadItems].sort(
        (a, b) =>
          new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime()
      )
      const subject =
        sorted.find((item) => item.subject?.trim())?.subject?.trim() ||
        sorted[0]?.body.slice(0, 40) ||
        rootEventId
      return { rootEventId, subject, items: sorted }
    })
    .sort((a, b) => {
      const aLast = a.items[a.items.length - 1]?.createdAt ?? ""
      const bLast = b.items[b.items.length - 1]?.createdAt ?? ""
      return new Date(bLast).getTime() - new Date(aLast).getTime()
    })
}
