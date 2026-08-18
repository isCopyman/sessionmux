import { describe, expect, it } from "vitest"

import {
  insertMentionToken,
  mentionAllFromText,
  mentionMarkdownForSession,
  removeMentionToken,
  roomMessageBodyParts,
  sessionIdsFromAtAliases,
} from "./room-message-body"

const members = [
  { conversationId: 3, title: "Session C" },
  { conversationId: 4, title: "Session D" },
]

const untitled = (id: number) => `Untitled #${id}`

describe("room message body", () => {
  it("keeps @all inside the sentence", () => {
    const parts = roomMessageBodyParts({
      body: "你们说说自己是谁 @all",
      members,
      mentionConversationIds: [3, 4],
      allLabel: "@all",
      humanLabel: "@human",
      untitled,
    })
    expect(parts).toEqual([
      { type: "text", value: "你们说说自己是谁 " },
      { type: "mention", kind: "all", label: "@all" },
    ])
  })

  it("renders a compact @sessionC alias as that Session", () => {
    const parts = roomMessageBodyParts({
      body: "帮我调研一下今天的hackernews @sessionC",
      members,
      mentionConversationIds: [],
      allLabel: "@all",
      humanLabel: "@human",
      untitled,
    })
    expect(parts.map((part) => (part.type === "mention" ? part.label : part.value))).toEqual([
      "帮我调研一下今天的hackernews ",
      "@Session C",
    ])
    expect(sessionIdsFromAtAliases("ping @sessionC", members, untitled)).toEqual([
      3,
    ])
  })

  it("folds a Session markdown link to an inline @", () => {
    const parts = roomMessageBodyParts({
      body: "ask [Session D](codeg://session/4) please",
      members,
      mentionConversationIds: [4],
      allLabel: "@all",
      humanLabel: "@human",
      untitled,
    })
    expect(parts).toEqual([
      { type: "text", value: "ask " },
      {
        type: "mention",
        kind: "session",
        conversationId: 4,
        label: "@Session D",
      },
      { type: "text", value: " please" },
    ])
  })

  it("appends structured mentions that never made it into the body", () => {
    const parts = roomMessageBodyParts({
      body: "你好",
      members,
      mentionConversationIds: [4],
      allLabel: "@all",
      humanLabel: "@human",
      untitled,
    })
    expect(parts).toEqual([
      { type: "text", value: "你好 " },
      {
        type: "mention",
        kind: "session",
        conversationId: 4,
        label: "@Session D",
      },
    ])
  })

  it("detects @all in free text and inserts mention tokens", () => {
    expect(mentionAllFromText("hello @all now")).toBe(true)
    expect(mentionAllFromText("hello")).toBe(false)
    expect(insertMentionToken("hello", "@all")).toBe("hello @all")
    expect(removeMentionToken("hello @all", "@all")).toBe("hello ")
    expect(mentionMarkdownForSession("Session C", 3)).toBe(
      "[Session C](codeg://session/3)"
    )
  })
})
