import { describe, expect, it } from "vitest"

import { canStartMentionAt } from "./mention-match"

function afterPrefix(prefix: string): boolean {
  return canStartMentionAt(`${prefix}@app`, prefix.length)
}

describe("canStartMentionAt", () => {
  it("allows text-node starts, whitespace, and CJK text", () => {
    expect(canStartMentionAt("@app", 0)).toBe(true)
    for (const prefix of [
      " ",
      "\n",
      "　",
      "请看",
      "テスト",
      "한글",
      "你好，",
    ]) {
      expect(afterPrefix(prefix)).toBe(true)
    }
  })

  it("allows CJK common marks and supplementary Han characters", () => {
    for (const prefix of ["ユーザー", "ヨミ・カナ", "玛丽·居里", "先祖𠀋"]) {
      expect(afterPrefix(prefix)).toBe(true)
    }
  })

  it("keeps email-like Latin prefixes blocked", () => {
    for (const prefix of ["me", "foo9", "a.b", "a_b", "a-b", "a+b"]) {
      expect(afterPrefix(prefix)).toBe(false)
    }
  })

  it("does not let a combining mark bypass the Latin-prefix guard", () => {
    expect(afterPrefix("a\u0323")).toBe(false)
    expect(afterPrefix("o\u0305")).toBe(false)
  })
})
