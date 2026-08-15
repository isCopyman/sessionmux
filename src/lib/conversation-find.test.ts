import { describe, expect, it } from "vitest"
import {
  findConversationMatches,
  nextConversationMatchIndex,
} from "@/lib/conversation-find"

describe("conversation find", () => {
  it("finds literal case-insensitive occurrences in transcript order", () => {
    expect(
      findConversationMatches(
        [
          { itemKey: "a", threadIndex: 2, text: "Alpha alpha" },
          { itemKey: "b", threadIndex: 5, text: "beta ALPHA" },
        ],
        "alpha"
      )
    ).toMatchObject([
      { itemKey: "a", threadIndex: 2, occurrenceIndex: 0, start: 0, end: 5 },
      { itemKey: "a", threadIndex: 2, occurrenceIndex: 1, start: 6, end: 11 },
      { itemKey: "b", threadIndex: 5, occurrenceIndex: 0, start: 5, end: 10 },
    ])
  })

  it("treats regex punctuation as ordinary text", () => {
    expect(
      findConversationMatches(
        [{ itemKey: "a", threadIndex: 0, text: "a+b then a.b" }],
        "a+b"
      )
    ).toHaveLength(1)
  })

  it("wraps next and previous navigation", () => {
    expect(nextConversationMatchIndex(2, 3, 1)).toBe(0)
    expect(nextConversationMatchIndex(0, 3, -1)).toBe(2)
    expect(nextConversationMatchIndex(0, 0, 1)).toBe(-1)
  })
})
