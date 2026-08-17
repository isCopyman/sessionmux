import { describe, expect, it } from "vitest"
import type { CollaborationDelivery, CollaborationFeed } from "@/lib/types"
import { mailboxSessionNeedsWorkbench } from "./use-open-queued-mailbox-sessions"

function delivery(
  overrides: Partial<CollaborationDelivery> = {}
): CollaborationDelivery {
  return {
    id: "d1",
    eventId: "e1",
    source: {
      conversationId: 1,
      title: "A",
      agentType: "codex",
      folderPath: "/",
      backend: "current",
    },
    target: {
      conversationId: 2,
      title: "B",
      agentType: "claude_code",
      folderPath: "/",
      backend: "current",
    },
    body: "hello",
    replyToEventId: null,
    expectsReply: false,
    replyReceived: false,
    urgency: "normal",
    invocationPolicy: "store_only",
    deliveryHint: "default",
    state: "pending",
    attentionState: "unread",
    openedAt: null,
    agentReceivedAt: null,
    agentReceiptKind: null,
    agentReceiptRef: null,
    obligationState: "none",
    obligationCreatedAt: null,
    obligationResolvedAt: null,
    attempts: 0,
    createdAt: "2026-08-18T00:00:00Z",
    updatedAt: "2026-08-18T00:00:00Z",
    ...overrides,
  }
}

function feed(inbound: CollaborationDelivery[]): CollaborationFeed {
  return {
    conversationId: 2,
    revision: 1,
    unreadCount: inbound.length,
    inbound,
    outbound: [],
  }
}

describe("mailboxSessionNeedsWorkbench", () => {
  it("opens a Session for queued invoke_when_idle mail", () => {
    expect(
      mailboxSessionNeedsWorkbench(
        feed([
          delivery({
            invocationPolicy: "invoke_when_idle",
            state: "queued",
          }),
        ])
      )
    ).toBe(true)
  })

  it("opens a Session for read-but-unreplied mail", () => {
    expect(
      mailboxSessionNeedsWorkbench(
        feed([
          delivery({
            obligationState: "awaiting_reply",
            replyReceived: false,
            agentReceivedAt: "2026-08-18T00:01:00Z",
          }),
        ])
      )
    ).toBe(true)
  })

  it("does not open for store_only mail with no reply obligation", () => {
    expect(mailboxSessionNeedsWorkbench(feed([delivery()]))).toBe(false)
  })
})
