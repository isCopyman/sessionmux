import type { CollaborationDelivery } from "@/lib/types"

export interface MailThread {
  rootEventId: string
  subject: string
  items: CollaborationDelivery[]
}

export function groupMailThreads(
  items: CollaborationDelivery[]
): MailThread[] {
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
