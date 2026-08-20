import { describe, expect, it } from "vitest"
import type { CollaborationDelivery } from "@/lib/types"
import {
  buildMailThreadTree,
  groupMailThreads,
  orderMailThreadByTree,
  summarizeMailThread,
} from "./mail-threads"

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

function treeIds(
  nodes: ReturnType<typeof buildMailThreadTree>
): Array<{ id: string; children: ReturnType<typeof treeIds> }> {
  return nodes.map((node) => ({
    id: node.delivery.eventId,
    children: treeIds(node.children),
  }))
}

describe("buildMailThreadTree", () => {
  it("nests follow-ups under the parent and sorts siblings by time", () => {
    const root = letter("e1", { createdAt: "2026-08-18T00:00:00Z" })
    const firstReply = letter("e2", {
      replyToEventId: "e1",
      createdAt: "2026-08-18T00:02:00Z",
    })
    const nested = letter("e3", {
      replyToEventId: "e2",
      createdAt: "2026-08-18T00:03:00Z",
    })
    const laterSibling = letter("e4", {
      replyToEventId: "e1",
      createdAt: "2026-08-18T00:04:00Z",
    })
    expect(
      treeIds(buildMailThreadTree([laterSibling, nested, root, firstReply]))
    ).toEqual([
      {
        id: "e1",
        children: [
          {
            id: "e2",
            children: [{ id: "e3", children: [] }],
          },
          { id: "e4", children: [] },
        ],
      },
    ])
  })

  it("treats a letter as a root when its parent is not in the mailbox view", () => {
    const orphan = letter("e9", {
      replyToEventId: "missing",
      createdAt: "2026-08-18T00:05:00Z",
    })
    expect(treeIds(buildMailThreadTree([orphan]))).toEqual([
      { id: "e9", children: [] },
    ])
  })

  it("flattens tree walk order without changing sibling time order", () => {
    const root = letter("e1", { createdAt: "2026-08-18T00:00:00Z" })
    const firstReply = letter("e2", {
      replyToEventId: "e1",
      createdAt: "2026-08-18T00:02:00Z",
    })
    const nested = letter("e3", {
      replyToEventId: "e2",
      createdAt: "2026-08-18T00:03:00Z",
    })
    const laterSibling = letter("e4", {
      replyToEventId: "e1",
      createdAt: "2026-08-18T00:04:00Z",
    })
    expect(
      orderMailThreadByTree([laterSibling, nested, root, firstReply]).map(
        (item) => item.eventId
      )
    ).toEqual(["e1", "e2", "e3", "e4"])
  })
})

describe("summarizeMailThread", () => {
  it("splits reply duty by side: needsReply is mine, awaitingReply is theirs", () => {
    // Seen from session 2: e1 arrived and owes a reply, e2 went back out on the
    // same chain and now waits for the peer.
    const inboundOwed = letter("e1", {
      obligationState: "awaiting_reply",
    })
    const outboundWaiting = letter("e2", {
      replyToEventId: "e1",
      obligationState: "awaiting_reply",
      source: {
        conversationId: 2,
        title: "B",
        agentType: "claude_code",
        folderPath: "/",
        backend: "current",
      },
      target: {
        conversationId: 1,
        title: "A",
        agentType: "codex",
        folderPath: "/",
        backend: "current",
      },
      createdAt: "2026-08-18T00:02:00Z",
    })
    const threads = groupMailThreads([inboundOwed, outboundWaiting])
    expect(threads).toHaveLength(1)
    const summary = summarizeMailThread(threads[0]!, 2)
    expect(summary.needsReply).toBe(true)
    expect(summary.awaitingReply).toBe(true)
    expect(summary.unreadCount).toBe(1)
    expect(summary.peerIds).toEqual([1])
    expect(summary.latestAt).toBe("2026-08-18T00:02:00Z")
  })

  it("counts only inbound letters the agent has not received as unread", () => {
    const receivedInbound = letter("e1", {
      agentReceivedAt: "2026-08-18T00:01:00Z",
    })
    const [thread] = groupMailThreads([receivedInbound])
    expect(summarizeMailThread(thread!, 2).unreadCount).toBe(0)
    expect(summarizeMailThread(thread!, 2).failed).toBe(false)
  })

  it("flags a failed delivery anywhere in the thread", () => {
    const failed = letter("e1", { state: "failed" })
    const [thread] = groupMailThreads([failed])
    expect(summarizeMailThread(thread!, 2).failed).toBe(true)
  })
})
