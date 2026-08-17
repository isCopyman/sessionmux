import type { CollaborationDelivery } from "@/lib/types"

/** Human-facing mailbox status. Delivery jargon is not shown as the primary chip. */
export type MailHumanStatus =
  | "failed"
  | "dismissed"
  | "unread"
  | "read_awaiting"
  | "replied"
  | "read"

export type MailDirection = "inbound" | "outbound" | "system"

/**
 * i18n key (Collaboration namespace) for a status chip. The same delivery
 * fact reads differently per side: an inbound chip talks about this session
 * ("needs reply"), an outbound chip talks about the recipient ("awaiting
 * reply"). Reusing one label for both is what made 已回复 ambiguous.
 */
export function mailStatusLabelKey(
  status: MailHumanStatus,
  direction: MailDirection
): string {
  if (direction === "outbound") {
    switch (status) {
      case "unread":
        return "mailOutUnread"
      case "read":
        return "mailOutRead"
      case "read_awaiting":
        return "mailOutAwaitingReply"
      case "replied":
        return "mailOutReplied"
      case "failed":
        return "mailFailed"
      case "dismissed":
        return "mailDismissed"
    }
  }
  switch (status) {
    case "unread":
      return "mailUnread"
    case "read":
      return "mailRead"
    case "read_awaiting":
      return "mailReadAwaitingReply"
    case "replied":
      return "mailReplied"
    case "failed":
      return "mailFailed"
    case "dismissed":
      return "mailDismissed"
  }
}

/** No open reply duty: FYI from birth, or later waived via no_reply_needed. */
export function mailNoReplyNeeded(delivery: CollaborationDelivery): boolean {
  if (!delivery.expectsReply) return true
  return delivery.obligationState === "resolved" && !delivery.replyReceived
}

export function mailHumanStatus(
  delivery: CollaborationDelivery
): MailHumanStatus {
  if (delivery.state === "failed") return "failed"
  if (delivery.state === "dismissed") return "dismissed"
  if (delivery.obligationState === "resolved" && delivery.replyReceived) {
    return "replied"
  }
  if (delivery.agentReceivedAt == null) return "unread"
  if (delivery.obligationState === "awaiting_reply") return "read_awaiting"
  return "read"
}

export function isAgentUnread(delivery: CollaborationDelivery): boolean {
  return (
    delivery.agentReceivedAt == null &&
    delivery.state !== "dismissed" &&
    delivery.state !== "failed"
  )
}

/** Shared mailbox colors: unread blue, awaiting amber, replied green. */
export function mailStatusVisual(status: MailHumanStatus): {
  bar: string
  chip: string
} {
  switch (status) {
    case "unread":
      return {
        bar: "bg-sky-500",
        chip: "bg-sky-500/15 text-sky-700 dark:text-sky-300",
      }
    case "read_awaiting":
      return {
        bar: "bg-amber-500",
        chip: "bg-amber-500/15 text-amber-800 dark:text-amber-300",
      }
    case "replied":
      return {
        bar: "bg-emerald-500",
        chip: "bg-emerald-500/15 text-emerald-800 dark:text-emerald-300",
      }
    case "failed":
      return {
        bar: "bg-destructive",
        chip: "bg-destructive/10 text-destructive",
      }
    case "dismissed":
      return {
        bar: "bg-muted-foreground/40",
        chip: "bg-muted text-muted-foreground",
      }
    case "read":
      return {
        bar: "bg-zinc-400 dark:bg-zinc-500",
        chip: "bg-muted text-muted-foreground",
      }
  }
}
