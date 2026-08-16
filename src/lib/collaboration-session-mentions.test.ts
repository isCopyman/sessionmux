import { describe, expect, it } from "vitest"

import {
  canSendCollaborationFromComposer,
  sessionIdsFromPromptBlocks,
  sessionIdsFromText,
  stripSessionMentions,
} from "./collaboration-session-mentions"

describe("collaboration session mentions", () => {
  it("collects unique numeric Session ids and skips the current Session", () => {
    expect(
      sessionIdsFromText(
        "ask [Reviewer](codeg://session/7) and [Writer](codeg://session/8) and [Reviewer](codeg://session/7)",
        7
      )
    ).toEqual([8])
  })

  it("ignores non-numeric or self Session tokens", () => {
    expect(
      sessionIdsFromText(
        "legacy [Old](codeg://session/codex_abc) and self [Me](codeg://session/3)",
        3
      )
    ).toEqual([])
  })

  it("reads Session ids out of prompt text blocks", () => {
    expect(
      sessionIdsFromPromptBlocks(
        [
          { type: "text", text: "see [A](codeg://session/2)" },
          { type: "text", text: "and [B](codeg://session/4)" },
        ],
        1
      )
    ).toEqual([2, 4])
  })

  it("keeps @ Session send available while the current Agent is prompting", () => {
    expect(
      canSendCollaborationFromComposer({
        sourceConversationId: 1,
        mentionedSessionIds: [2],
        isEditingQueueItem: false,
      })
    ).toBe(true)
    expect(
      canSendCollaborationFromComposer({
        sourceConversationId: 1,
        mentionedSessionIds: [2],
        isEditingQueueItem: true,
      })
    ).toBe(false)
  })

  it("strips Session badges so the delivery body is the user's words", () => {
    expect(
      stripSessionMentions(
        "Please review this claim [Reviewer](codeg://session/9)\n\nkeep the path src/lib/foo.ts"
      )
    ).toBe("Please review this claim\n\nkeep the path src/lib/foo.ts")
  })
})
