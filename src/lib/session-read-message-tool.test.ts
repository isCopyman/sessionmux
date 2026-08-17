import { describe, expect, it } from "vitest"
import {
  isSessionReadMessageToolName,
  parseSessionReadMessageOutput,
} from "./session-read-message-tool"

describe("session read_message tool", () => {
  it("recognizes host-prefixed names", () => {
    expect(isSessionReadMessageToolName("read_message")).toBe(true)
    expect(
      isSessionReadMessageToolName("mcp__codeg-mcp__read_message")
    ).toBe(true)
  })

  it("reads title and body from structured output", () => {
    expect(
      parseSessionReadMessageOutput(
        JSON.stringify({
          event_id: "b80f5bea-2dd6-41b5-8a07-b68d49fe269a",
          from_session_id: 290,
          from_title: "Session C",
          title: "Need review",
          body: "please check claim 3",
          expects_reply: true,
        })
      )
    ).toEqual({
      eventId: "b80f5bea-2dd6-41b5-8a07-b68d49fe269a",
      fromSessionId: 290,
      fromTitle: "Session C",
      title: "Need review",
      body: "please check claim 3",
      expectsReply: true,
    })
  })

  it("reads title and body from companion text or MCP wrapper", () => {
    const text = [
      "Opened letter b80f5bea-2dd6-41b5-8a07-b68d49fe269a from 290 Session C.",
      "Title: Need review",
      "This letter expects a reply.",
      "--- message ---",
      "please check claim 3",
    ].join("\n")
    expect(parseSessionReadMessageOutput(text)).toMatchObject({
      eventId: "b80f5bea-2dd6-41b5-8a07-b68d49fe269a",
      title: "Need review",
      body: "please check claim 3",
      expectsReply: true,
    })
    expect(
      parseSessionReadMessageOutput(
        JSON.stringify({
          content: [{ type: "text", text }],
        })
      )
    ).toMatchObject({
      title: "Need review",
      body: "please check claim 3",
    })
  })
})
