import { describe, expect, it } from "vitest"

import {
  isSessionSendMessageToolName,
  parseSessionSendMessageEventId,
  parseSessionSendMessageInput,
} from "./session-send-message-tool"

describe("session send_message tool", () => {
  it("recognizes host-prefixed MCP names", () => {
    expect(isSessionSendMessageToolName("send_message")).toBe(true)
    expect(isSessionSendMessageToolName("mcp__codeg-mcp__send_message")).toBe(
      true
    )
  })

  it("reads the event id from a send_message result", () => {
    expect(
      parseSessionSendMessageEventId(
        JSON.stringify({ accepted: true, event_id: "b80f5bea-2dd6-41b5-8a07-b68d49fe269a" })
      )
    ).toBe("b80f5bea-2dd6-41b5-8a07-b68d49fe269a")
    expect(
      parseSessionSendMessageEventId(
        "Message accepted as event b80f5bea-2dd6-41b5-8a07-b68d49fe269a."
      )
    ).toBe("b80f5bea-2dd6-41b5-8a07-b68d49fe269a")
  })

  it("reads target ids and content from the MCP arguments", () => {
    expect(
      parseSessionSendMessageInput(
        JSON.stringify({
          target_session_ids: [287],
          content: "123",
        })
      )
    ).toEqual({ targetSessionIds: [287], title: "", content: "123" })
  })
})
