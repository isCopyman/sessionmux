import type { CollaborationDelivery } from "@/lib/types"

/** Human-facing mailbox status. Delivery jargon is not shown as the primary chip. */
export type MailHumanStatus =
  | "failed"
  | "dismissed"
  | "unread"
  | "read_awaiting"
  | "replied"
  | "read"

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
