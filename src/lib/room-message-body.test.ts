import { describe, expect, it } from "vitest"

import { sessionIdsFromText } from "./collaboration-session-mentions"
import {
  insertMentionToken,
  mentionAllFromText,
  mentionMarkdownForSession,
  removeMentionToken,
  roomMessageBodyParts,
  roomMessagePlainText,
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
    expect(
      parts.map((part) => (part.type === "text" ? part.value : part.label))
    ).toEqual(["帮我调研一下今天的hackernews ", "@Session C"])
    expect(
      sessionIdsFromAtAliases("ping @sessionC", members, untitled)
    ).toEqual([3])
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

  it("treats the codeg://all URI as a structured wake-everyone token", () => {
    // The badge label is localized, so detection keys on the URI alone.
    expect(mentionAllFromText("[@alle](codeg://all)")).toBe(true)
    expect(mentionAllFromText("[@tous](codeg://all) hi")).toBe(true)
    expect(mentionAllFromText("codeg://allowed")).toBe(false)

    const parts = roomMessageBodyParts({
      body: "各位 [@alle](codeg://all) 看看",
      members,
      mentionConversationIds: [],
      allLabel: "@all",
      humanLabel: "@human",
      untitled,
    })
    expect(parts).toEqual([
      { type: "text", value: "各位 " },
      { type: "mention", kind: "all", label: "@all" },
      { type: "text", value: " 看看" },
    ])
  })

  it("renders a file reference badge inline without disturbing mention parsing", () => {
    const parts = roomMessageBodyParts({
      body: "see [@foo.ts](file:///repo/src/foo.ts) please @all",
      members,
      mentionConversationIds: [],
      allLabel: "@all",
      humanLabel: "@human",
      untitled,
    })
    expect(parts).toEqual([
      { type: "text", value: "see " },
      {
        type: "reference",
        refType: "file",
        label: "@foo.ts",
        uri: "file:///repo/src/foo.ts",
      },
      { type: "text", value: " please " },
      { type: "mention", kind: "all", label: "@all" },
    ])
  })

  it("renders a commit reference badge inline", () => {
    const parts = roomMessageBodyParts({
      body: "landed in [a1b2c3d](codeg://commit/%2Frepo@a1b2c3ddeadbeef)",
      members,
      mentionConversationIds: [],
      allLabel: "@all",
      humanLabel: "@human",
      untitled,
    })
    expect(parts).toEqual([
      { type: "text", value: "landed in " },
      {
        type: "reference",
        refType: "commit",
        label: "a1b2c3d",
        uri: "codeg://commit/%2Frepo@a1b2c3ddeadbeef",
      },
    ])
  })

  it("leaves an unrecognized reference kind (e.g. an agent link) as raw text", () => {
    const parts = roomMessageBodyParts({
      body: "ping [@Codex](codeg://agent/codex) now",
      members,
      mentionConversationIds: [],
      allLabel: "@all",
      humanLabel: "@human",
      untitled,
    })
    expect(parts).toEqual([
      { type: "text", value: "ping [@Codex](codeg://agent/codex) now" },
    ])
  })

  it("projects a post to searchable text with chip labels, not link syntax", () => {
    expect(
      roomMessagePlainText({
        body: "ask [Session D](codeg://session/4) about the plan",
        members,
        mentionConversationIds: [],
        allLabel: "@all",
        humanLabel: "@human",
        untitled,
      })
    ).toBe("ask @Session D about the plan")
  })

  it("projects a mention recovered from metadata, which the raw body lacks", () => {
    const text = roomMessagePlainText({
      body: "please review",
      members,
      mentionConversationIds: [3],
      mentionHuman: true,
      allLabel: "@all",
      humanLabel: "@human",
      untitled,
    })
    // Appended chips run together exactly as `roomMessageMarkdown` emits them
    // (adjacent badges, no separator of their own).
    expect(text).toBe("please review @Session C@human")
  })

  it("does not let a file reference contribute a session id or affect the wake list", () => {
    const body = "check [@foo.ts](file:///repo/src/foo.ts) @all"
    // sessionIdsFromText matches only `codeg://session/<id>` — a file:// uri
    // must never be mistaken for a session mention.
    expect(sessionIdsFromText(body)).toEqual([])
    expect(sessionIdsFromAtAliases(body, members, untitled)).toEqual([])
    expect(mentionAllFromText(body)).toBe(true)
  })
})
