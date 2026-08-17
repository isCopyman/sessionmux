import { describe, expect, it } from "vitest"
import type { CollaborationDelivery } from "@/lib/types"
import { groupMailThreads } from "./mail-threads"

function letter(
  eventId: string,
  overrides: Partial<CollaborationDelivery> = {}
): CollaborationDelivery {
  return {
    id: eventId,
    eventId,
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
    subject: overrides.subject ?? "Ping",
    body: "body",
    replyToEventId: null,
    expectsReply: true,
    replyReceived: false,
    urgency: "normal",
    invocationPolicy: "invoke_when_idle",
    deliveryHint: "default",
    state: "embedded",
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

describe("groupMailThreads", () => {
  it("groups a reply under the original letter", () => {
    const root = letter("e1", {
      subject: "Need review",
      createdAt: "2026-08-18T00:00:00Z",
    })
    const reply = letter("e2", {
      subject: "Re: Need review",
      replyToEventId: "e1",
      createdAt: "2026-08-18T00:01:00Z",
    })
    const threads = groupMailThreads([reply, root])
    expect(threads).toHaveLength(1)
    expect(threads[0]?.rootEventId).toBe("e1")
    expect(threads[0]?.subject).toBe("Need review")
    expect(threads[0]?.items.map((item) => item.eventId)).toEqual(["e1", "e2"])
  })
})
