import { describe, expect, it } from "vitest"

import {
  decorateDeleteAddBlock,
  diffTokens,
  pairDeleteAddLines,
  tokenizeForIntraline,
} from "./diff-intraline"

describe("diff-intraline", () => {
  it("tokenizes words, whitespace, and CJK characters separately", () => {
    expect(tokenizeForIntraline("modified: 时间")).toEqual([
      "modified",
      ":",
      " ",
      "时",
      "间",
    ])
  })

  it("highlights only the replaced timestamp fragment", () => {
    const diff = diffTokens(
      "modified: 2026-08-15T08:05:08.980Z",
      "modified: 2026-08-15T08:12:35.069Z"
    )
    expect(diff).not.toBeNull()
    const removed = diff!.oldSpans
      .filter((span) => span.kind === "removed")
      .map((span) => span.text)
      .join("")
    const added = diff!.newSpans
      .filter((span) => span.kind === "added")
      .map((span) => span.text)
      .join("")
    expect(removed).toBe("0508980Z")
    expect(added).toBe("1235069Z")
    expect(diff!.oldSpans.some((span) => span.kind === "equal")).toBe(true)
    expect(diff!.oldSpans.map((span) => span.text).join("")).toBe(
      "modified: 2026-08-15T08:05:08.980Z"
    )
    expect(diff!.newSpans.map((span) => span.text).join("")).toBe(
      "modified: 2026-08-15T08:12:35.069Z"
    )
  })

  it("falls back to character-level comparison for Chinese phrases", () => {
    const diff = diffTokens("请打开工作台", "请关闭工作台")
    expect(diff).not.toBeNull()
    expect(
      diff!.oldSpans
        .filter((span) => span.kind === "removed")
        .map((span) => span.text)
        .join("")
    ).toBe("打开")
    expect(
      diff!.newSpans
        .filter((span) => span.kind === "added")
        .map((span) => span.text)
        .join("")
    ).toBe("关闭")
  })

  it("does not invent dark spans when two lines share no useful prefix", () => {
    expect(diffTokens("alpha", "omega")).toBeNull()
  })

  it("pairs a one-to-one replacement and a bounded multi-line block", () => {
    expect(pairDeleteAddLines(["const foo = 1"], ["const foo = 2"])).toEqual([
      { deletedIndex: 0, addedIndex: 0 },
    ])

    expect(
      pairDeleteAddLines(
        ["keep left", "change me", "unrelated old"],
        ["keep left", "change you"]
      )
    ).toEqual([
      { deletedIndex: 0, addedIndex: 0 },
      { deletedIndex: 1, addedIndex: 1 },
    ])
  })

  it("decorates only the paired replacement inside a delete/add block", () => {
    const decorated = decorateDeleteAddBlock(
      ["title: Session A", "orphan deleted"],
      ["title: Session B"]
    )
    expect(decorated.deletedSpans[0]).not.toBeNull()
    expect(decorated.deletedSpans[1]).toBeNull()
    expect(
      decorated.deletedSpans[0]
        ?.filter((span) => span.kind === "removed")
        .map((span) => span.text)
        .join("")
    ).toBe("A")
    expect(
      decorated.addedSpans[0]
        ?.filter((span) => span.kind === "added")
        .map((span) => span.text)
        .join("")
    ).toBe("B")
  })
})
