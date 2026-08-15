import { describe, expect, it } from "vitest"
import {
  clientPointFromDrag,
  dropIndexFromMidpoints,
  moveIdToDropIndex,
  splitDropEdgeFromPoint,
  translatedRectCenter,
} from "./tab-drag-drop"

describe("dropIndexFromMidpoints", () => {
  const midpoints = [100, 200, 300]

  it("drops before the first tab when the cursor is left of every midpoint", () => {
    expect(dropIndexFromMidpoints(50, midpoints)).toBe(0)
  })

  it("drops between tabs by midpoint comparison", () => {
    expect(dropIndexFromMidpoints(150, midpoints)).toBe(1)
    expect(dropIndexFromMidpoints(250, midpoints)).toBe(2)
  })

  it("appends when the cursor is right of every midpoint", () => {
    expect(dropIndexFromMidpoints(350, midpoints)).toBe(3)
  })

  it("returns 0 for an empty strip", () => {
    expect(dropIndexFromMidpoints(123, [])).toBe(0)
  })
})

describe("moveIdToDropIndex", () => {
  const ids = ["a", "b", "c"]

  it("commits a rightward move after discounting the source slot", () => {
    expect(moveIdToDropIndex(ids, "a", 3)).toEqual(["b", "c", "a"])
  })

  it("commits a leftward move at the requested insertion", () => {
    expect(moveIdToDropIndex(ids, "c", 0)).toEqual(["c", "a", "b"])
  })

  it("preserves identity and order for a same-slot or unknown drag", () => {
    expect(moveIdToDropIndex(ids, "b", 2)).toEqual(ids)
    expect(moveIdToDropIndex(ids, "missing", 1)).toBe(ids)
  })
})

describe("clientPointFromDrag", () => {
  it("prefers the event's own client coordinates", () => {
    expect(
      clientPointFromDrag(
        { clientX: 12, clientY: 34 },
        { point: { x: 999, y: 999 } }
      )
    ).toEqual({ x: 12, y: 34 })
  })

  it("falls back to page coords minus scroll when the event has none", () => {
    // jsdom: scrollX/scrollY are 0.
    expect(clientPointFromDrag({}, { point: { x: 40, y: 50 } })).toEqual({
      x: 40,
      y: 50,
    })
    expect(clientPointFromDrag(null, { point: { x: 7, y: 8 } })).toEqual({
      x: 7,
      y: 8,
    })
  })
})

describe("translatedRectCenter", () => {
  it("tracks the dragged item's center instead of the grab point", () => {
    expect(
      translatedRectCenter(
        { left: 100, top: 50, width: 200, height: 40 },
        { x: 35, y: 90 }
      )
    ).toEqual({ x: 235, y: 160 })
  })
})

describe("splitDropEdgeFromPoint", () => {
  const rect = { left: 100, top: 50, width: 400, height: 200 }

  it("maps the four edge bands to split directions", () => {
    expect(splitDropEdgeFromPoint(110, 150, rect)).toBe("left")
    expect(splitDropEdgeFromPoint(490, 150, rect)).toBe("right")
    expect(splitDropEdgeFromPoint(300, 55, rect)).toBe("up")
    expect(splitDropEdgeFromPoint(300, 245, rect)).toBe("down")
  })

  it("keeps the pane center as an ordinary move target", () => {
    expect(splitDropEdgeFromPoint(300, 150, rect)).toBeNull()
  })

  it("uses a centered 40% join zone and assigns the surrounding area to a split", () => {
    // 28% from the left used to miss the old 22% edge strip. It is outside
    // the centered 40% x 40% join zone, so the nearest split is now acquired.
    expect(splitDropEdgeFromPoint(212, 150, rect)).toBe("left")
    // 32% is inside the join zone when no edge is currently active.
    expect(splitDropEdgeFromPoint(228, 150, rect)).toBeNull()
  })

  it("adds hysteresis when leaving an active edge preview", () => {
    expect(
      splitDropEdgeFromPoint(228, 150, rect, { currentEdge: "left" })
    ).toBe("left")
    expect(
      splitDropEdgeFromPoint(244, 150, rect, { currentEdge: "left" })
    ).toBeNull()
  })

  it("chooses the nearest normalized edge at a corner", () => {
    // 5% from the left, 10% from the top.
    expect(splitDropEdgeFromPoint(120, 70, rect)).toBe("left")
    // 20% from the left, 2.5% from the top.
    expect(splitDropEdgeFromPoint(180, 55, rect)).toBe("up")
  })

  it("rejects points outside or degenerate geometry", () => {
    expect(splitDropEdgeFromPoint(99, 150, rect)).toBeNull()
    expect(splitDropEdgeFromPoint(100, 50, { ...rect, width: 0 })).toBeNull()
  })
})
