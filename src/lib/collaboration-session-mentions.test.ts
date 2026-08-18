import { describe, expect, it } from "vitest"

import {
  mentionsHumanFromText,
  sessionIdsFromPromptBlocks,
  sessionIdsFromText,
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

  it("treats only structured human URIs as @human", () => {
    expect(mentionsHumanFromText("ping @user and @alice")).toBe(false)
    expect(mentionsHumanFromText("see [You](codeg://human)")).toBe(true)
    expect(mentionsHumanFromText("see [You](codeg://user)")).toBe(true)
  })
})
