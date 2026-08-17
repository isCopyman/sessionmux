import { describe, expect, it } from "vitest"

import {
  isSessionSendMessageToolName,
  parseSessionSendMessageInput,
} from "./session-send-message-tool"

describe("session send_message tool", () => {
  it("recognizes host-prefixed MCP names", () => {
    expect(isSessionSendMessageToolName("send_message")).toBe(true)
    expect(isSessionSendMessageToolName("mcp__codeg-mcp__send_message")).toBe(
      true
    )
  })

  it("reads target ids and content from the MCP arguments", () => {
    expect(
      parseSessionSendMessageInput(
        JSON.stringify({
          target_session_ids: [287],
          content: "123",
        })
      )
    ).toEqual({ targetSessionIds: [287], content: "123" })
  })
})
