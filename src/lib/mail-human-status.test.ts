import { describe, expect, it } from "vitest"
import type { CollaborationDelivery } from "@/lib/types"
import {
  isAgentUnread,
  mailHumanStatus,
  mailNoReplyNeeded,
  mailStatusLabelKey,
  mailStatusVisual,
} from "./mail-human-status"

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

describe("mailHumanStatus", () => {
  it("treats notify-injected mail as unread until the Agent reads it", () => {
    expect(mailHumanStatus(delivery({ state: "embedded" }))).toBe("unread")
    expect(isAgentUnread(delivery({ state: "embedded" }))).toBe(true)
  })

  it("marks read-but-unreplied after receipt while a reply is still owed", () => {
    expect(
      mailHumanStatus(
        delivery({
          agentReceivedAt: "2026-08-18T00:01:00Z",
          obligationState: "awaiting_reply",
          expectsReply: true,
        })
      )
    ).toBe("read_awaiting")
  })

  it("gives unread, awaiting, and replied distinct color tokens", () => {
    expect(mailStatusVisual("unread").bar).toContain("sky")
    expect(mailStatusVisual("read_awaiting").bar).toContain("amber")
    expect(mailStatusVisual("replied").bar).toContain("emerald")
    expect(mailStatusVisual("failed").chip).toContain("destructive")
  })

  it("marks replied only when a linked reply exists", () => {
    expect(
      mailHumanStatus(
        delivery({
          agentReceivedAt: "2026-08-18T00:01:00Z",
          obligationState: "resolved",
          replyReceived: true,
        })
      )
    ).toBe("replied")
  })
})

describe("mailStatusLabelKey", () => {
  it("mirrors every reply-cycle status by direction so 已回复 is never ambiguous", () => {
    expect(mailStatusLabelKey("unread", "inbound")).toBe("mailUnread")
    expect(mailStatusLabelKey("unread", "outbound")).toBe("mailOutUnread")
    expect(mailStatusLabelKey("read", "inbound")).toBe("mailRead")
    expect(mailStatusLabelKey("read", "outbound")).toBe("mailOutRead")
    expect(mailStatusLabelKey("read_awaiting", "inbound")).toBe(
      "mailReadAwaitingReply"
    )
    expect(mailStatusLabelKey("read_awaiting", "outbound")).toBe(
      "mailOutAwaitingReply"
    )
    expect(mailStatusLabelKey("replied", "inbound")).toBe("mailReplied")
    expect(mailStatusLabelKey("replied", "outbound")).toBe("mailOutReplied")
  })

  it("keeps terminal delivery failures direction-neutral", () => {
    expect(mailStatusLabelKey("failed", "inbound")).toBe("mailFailed")
    expect(mailStatusLabelKey("failed", "outbound")).toBe("mailFailed")
    expect(mailStatusLabelKey("dismissed", "outbound")).toBe("mailDismissed")
  })
})

describe("mailNoReplyNeeded", () => {
  it("is true for FYI mail and for obligations waived without a reply", () => {
    expect(mailNoReplyNeeded(delivery({ expectsReply: false }))).toBe(true)
    expect(
      mailNoReplyNeeded(
        delivery({
          expectsReply: true,
          obligationState: "resolved",
          replyReceived: false,
        })
      )
    ).toBe(true)
  })

  it("is false while a reply is owed or after a real reply arrived", () => {
    expect(
      mailNoReplyNeeded(
        delivery({ expectsReply: true, obligationState: "awaiting_reply" })
      )
    ).toBe(false)
    expect(
      mailNoReplyNeeded(
        delivery({
          expectsReply: true,
          obligationState: "resolved",
          replyReceived: true,
        })
      )
    ).toBe(false)
  })
})
