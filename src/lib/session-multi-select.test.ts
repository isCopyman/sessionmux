import { describe, expect, it } from "vitest"

import {
  nextSessionSelection,
  rangeSessionIds,
  sessionClickIntent,
  uniqueOrderedIds,
} from "./session-multi-select"

const items = [
  { id: 1, title: "a" },
  { id: 2, title: "b" },
  { id: 3, title: "c" },
  { id: 4, title: "d" },
]
const ordered = items.map((item) => item.id)
const lookup = (id: number) => items.find((item) => item.id === id)

describe("sessionClickIntent", () => {
  it("opens on a plain click", () => {
    expect(sessionClickIntent({})).toBe("open")
  })

  it("toggles on ctrl or meta", () => {
    expect(sessionClickIntent({ ctrlKey: true })).toBe("toggle")
    expect(sessionClickIntent({ metaKey: true })).toBe("toggle")
  })

  it("ranges on shift even when a modifier is also held", () => {
    expect(sessionClickIntent({ shiftKey: true, ctrlKey: true })).toBe("range")
  })
})

describe("uniqueOrderedIds", () => {
  it("keeps first-seen order and drops later copies", () => {
    expect(uniqueOrderedIds([2, 1, 2, 3, 1])).toEqual([2, 1, 3])
  })
})

describe("rangeSessionIds", () => {
  it("selects an inclusive span from the anchor", () => {
    expect(rangeSessionIds(ordered, 2, 4)).toEqual([2, 3, 4])
    expect(rangeSessionIds(ordered, 4, 2)).toEqual([2, 3, 4])
  })

  it("falls back to the clicked id when there is no usable anchor", () => {
    expect(rangeSessionIds(ordered, null, 3)).toEqual([3])
    expect(rangeSessionIds(ordered, 9, 3)).toEqual([3])
  })
})

describe("nextSessionSelection", () => {
  it("clears on open so a regular click leaves multi-select", () => {
    const current = new Map([[1, items[0]!]])
    expect(
      nextSessionSelection(current, ordered, lookup, items[2]!, "open", 1)
    ).toEqual({
      selected: new Map(),
      anchorId: 3,
    })
  })

  it("toggles membership without dropping the rest of the set", () => {
    const current = new Map([[1, items[0]!]])
    const added = nextSessionSelection(
      current,
      ordered,
      lookup,
      items[2]!,
      "toggle",
      1
    )
    expect([...added.selected.keys()]).toEqual([1, 3])
    const removed = nextSessionSelection(
      added.selected,
      ordered,
      lookup,
      items[0]!,
      "toggle",
      3
    )
    expect([...removed.selected.keys()]).toEqual([3])
  })

  it("adds an inclusive range from the last anchor", () => {
    const current = new Map([[2, items[1]!]])
    const next = nextSessionSelection(
      current,
      ordered,
      lookup,
      items[3]!,
      "range",
      2
    )
    expect([...next.selected.keys()]).toEqual([2, 3, 4])
    expect(next.anchorId).toBe(2)
  })
})
