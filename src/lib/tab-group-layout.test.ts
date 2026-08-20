import { describe, expect, it } from "vitest"
import {
  computeRects,
  firstLeafId,
  isLayoutNode,
  leafIds,
  MIN_SPLIT_RATIO,
  neighborGroupId,
  normalizeTree,
  parentSplitOf,
  reanchorForContainerResize,
  removeGroup,
  resizeSplitAt,
  singleGroupLayout,
  splitGroup,
  toggleOrientation,
  type LayoutNode,
  type SplitNode,
} from "./tab-group-layout"

const leaf = (id: string): LayoutNode => ({ type: "group", id })

/** Pane widths in pixels for a column of `columnPx` — ratios only become a bug
 *  report once you read them as absolute sizes. */
function paneWidths(
  tree: LayoutNode,
  columnPx: number
): Record<string, number> {
  const widths: Record<string, number> = {}
  for (const [id, rect] of computeRects(tree).groups) {
    widths[id] = (rect.w / 100) * columnPx
  }
  return widths
}

/** Horizontal [a | b | c], equal thirds. */
function threeEqualPanes(): SplitNode {
  return {
    type: "split",
    id: "s-root",
    orientation: "horizontal",
    children: [leaf("a"), leaf("b"), leaf("c")],
    ratios: [1 / 3, 1 / 3, 1 / 3],
  }
}

describe("splitGroup", () => {
  it("replaces a root leaf with a binary split", () => {
    const tree = splitGroup(leaf("a"), "a", "right", "b")
    expect(tree).toEqual({
      type: "split",
      id: "s-b",
      orientation: "horizontal",
      children: [leaf("a"), leaf("b")],
      ratios: [0.5, 0.5],
    })
  })

  it("maps down to a vertical orientation", () => {
    const tree = splitGroup(leaf("a"), "a", "down", "b") as SplitNode
    expect(tree.orientation).toBe("vertical")
  })

  it("places left/up splits before the target group", () => {
    const left = splitGroup(leaf("a"), "a", "left", "b") as SplitNode
    expect(left.orientation).toBe("horizontal")
    expect(leafIds(left)).toEqual(["b", "a"])

    const up = splitGroup(leaf("a"), "a", "up", "b") as SplitNode
    expect(up.orientation).toBe("vertical")
    expect(leafIds(up)).toEqual(["b", "a"])
  })

  it("flattens a same-orientation split into a sibling insert", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const three = splitGroup(two, "a", "right", "c") as SplitNode
    expect(three.orientation).toBe("horizontal")
    expect(leafIds(three)).toEqual(["a", "c", "b"])
    // "a" halves its own share; "b" keeps its half.
    expect(three.ratios).toEqual([0.25, 0.25, 0.5])
  })

  it("flattens a same-orientation left split before its target", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const three = splitGroup(two, "b", "left", "c") as SplitNode
    expect(leafIds(three)).toEqual(["a", "c", "b"])
    expect(three.ratios).toEqual([0.5, 0.25, 0.25])
  })

  it("nests a cross-orientation split", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const nested = splitGroup(two, "a", "down", "c") as SplitNode
    expect(nested.orientation).toBe("horizontal")
    expect(nested.children[0]).toEqual({
      type: "split",
      id: "s-c",
      orientation: "vertical",
      children: [leaf("a"), leaf("c")],
      ratios: [0.5, 0.5],
    })
    expect(leafIds(nested)).toEqual(["a", "c", "b"])
  })

  it("returns the same reference for an unknown group", () => {
    const tree = splitGroup(leaf("a"), "a", "right", "b")
    expect(splitGroup(tree, "missing", "right", "c")).toBe(tree)
  })
})

describe("removeGroup / normalizeTree", () => {
  it("collapses a binary split back to a single leaf", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    expect(removeGroup(two, "b")).toEqual(leaf("a"))
  })

  it("keeps a flattened split when one of three leaves goes", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const three = splitGroup(two, "a", "right", "c")
    const next = removeGroup(three, "c") as SplitNode
    expect(leafIds(next)).toEqual(["a", "b"])
    // Ratios renormalize to sum 1.
    expect(next.ratios.reduce((s, r) => s + r, 0)).toBeCloseTo(1)
  })

  it("merges same-orientation nesting exposed by a removal", () => {
    // horizontal [ vertical [a, c], b ] — removing c leaves horizontal [a, b].
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const nested = splitGroup(two, "a", "down", "c")
    const next = removeGroup(nested, "c") as SplitNode
    expect(next.type).toBe("split")
    expect(next.orientation).toBe("horizontal")
    expect(leafIds(next)).toEqual(["a", "b"])
  })

  it("refuses to remove the last leaf", () => {
    const tree = leaf("a")
    expect(removeGroup(tree, "a")).toBe(tree)
  })

  it("normalizeTree drops dead leaves and falls back to the first leaf", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    expect(normalizeTree(two, new Set(["a"]))).toEqual(leaf("a"))
    expect(normalizeTree(two, new Set())).toEqual(leaf("a"))
    expect(normalizeTree(two, new Set(["a", "b"]))).toBe(two)
  })
})

describe("toggleOrientation", () => {
  it("flips the parent split of the group", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const flipped = toggleOrientation(two, "a") as SplitNode
    expect(flipped.orientation).toBe("vertical")
    expect(leafIds(flipped)).toEqual(["a", "b"])
  })

  it("is a no-op on a root leaf", () => {
    const tree = leaf("a")
    expect(toggleOrientation(tree, "a")).toBe(tree)
  })

  it("merges with the parent when the flip aligns orientations", () => {
    // horizontal [ vertical [a, c], b ] → flip [a, c] to horizontal →
    // one flat horizontal [a, c, b].
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const nested = splitGroup(two, "a", "down", "c")
    const flipped = toggleOrientation(nested, "a") as SplitNode
    expect(flipped.type).toBe("split")
    expect(flipped.orientation).toBe("horizontal")
    expect(flipped.children.every((c) => c.type === "group")).toBe(true)
    expect(leafIds(flipped)).toEqual(["a", "c", "b"])
  })
})

describe("neighborGroupId / parentSplitOf / firstLeafId", () => {
  it("prefers the previous leaf, else the next", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const three = splitGroup(two, "a", "right", "c")
    expect(neighborGroupId(three, "c")).toBe("a")
    expect(neighborGroupId(three, "a")).toBe("c")
    expect(neighborGroupId(leaf("a"), "a")).toBeNull()
  })

  it("finds the direct parent split", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const nested = splitGroup(two, "a", "down", "c")
    expect(parentSplitOf(nested, "a")?.id).toBe("s-c")
    expect(parentSplitOf(nested, "b")?.id).toBe("s-b")
    expect(parentSplitOf(leaf("a"), "a")).toBeNull()
    expect(firstLeafId(nested)).toBe("a")
  })
})

describe("resizeSplitAt", () => {
  it("moves the boundary within the pair", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const next = resizeSplitAt(two, "s-b", 0, 0.3) as SplitNode
    expect(next.ratios[0]).toBeCloseTo(0.3)
    expect(next.ratios[1]).toBeCloseTo(0.7)
  })

  it("clamps each side to the minimum share", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const next = resizeSplitAt(two, "s-b", 0, 0.01) as SplitNode
    expect(next.ratios[0]).toBeCloseTo(0.15)
  })

  it("leaves non-adjacent siblings untouched", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const three = splitGroup(two, "a", "right", "c")
    const splitId = (three as SplitNode).id
    const next = resizeSplitAt(three, splitId, 0, 0.3) as SplitNode
    expect(next.ratios[2]).toBeCloseTo(0.5)
    expect(next.ratios[0] + next.ratios[1]).toBeCloseTo(0.5)
    expect(next.ratios[0]).toBeCloseTo(0.3)
  })

  it("ignores unknown split ids and out-of-range handles", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    expect(resizeSplitAt(two, "nope", 0, 0.3)).toBe(two)
    expect(resizeSplitAt(two, "s-b", 5, 0.3)).toBe(two)
  })

  it("keeps the far pane's ABSOLUTE width on a middle-divider drag", () => {
    // Three equal panes in a 300px column; drag the b|c boundary right.
    const next = resizeSplitAt(threeEqualPanes(), "s-root", 1, 0.75)
    const widths = paneWidths(next, 300)
    expect(widths.a).toBeCloseTo(100)
    expect(widths.b).toBeCloseTo(125)
    expect(widths.c).toBeCloseTo(75)
  })
})

describe("reanchorForContainerResize", () => {
  it("gives the whole shrink to the pane against the moved edge", () => {
    // The reported O20 repro: a file column on the right, two session panes on
    // the left. Narrowing the conversation column from 300px to 240px must
    // only eat into the pane that touches the files divider.
    const next = reanchorForContainerResize(
      threeEqualPanes(),
      "horizontal",
      "end",
      300 / 240
    )
    const widths = paneWidths(next, 240)
    expect(widths.a).toBeCloseTo(100)
    expect(widths.b).toBeCloseTo(100)
    expect(widths.c).toBeCloseTo(40)
  })

  it("gives the whole growth to the pane against the moved edge", () => {
    const next = reanchorForContainerResize(
      threeEqualPanes(),
      "horizontal",
      "end",
      300 / 380
    )
    const widths = paneWidths(next, 380)
    expect(widths.a).toBeCloseTo(100)
    expect(widths.b).toBeCloseTo(100)
    expect(widths.c).toBeCloseTo(180)
  })

  it("absorbs from the leading pane for a start-edge divider", () => {
    const next = reanchorForContainerResize(
      threeEqualPanes(),
      "horizontal",
      "start",
      300 / 240
    )
    const widths = paneWidths(next, 240)
    expect(widths.a).toBeCloseTo(40)
    expect(widths.b).toBeCloseTo(100)
    expect(widths.c).toBeCloseTo(100)
  })

  it("floors the absorbing pane at the minimum share", () => {
    // 300px → 200px is more than pane c can give up on its own.
    const next = reanchorForContainerResize(
      threeEqualPanes(),
      "horizontal",
      "end",
      300 / 200
    ) as SplitNode
    expect(next.ratios[2]).toBeCloseTo(MIN_SPLIT_RATIO)
    // Past the floor the column has simply run out of room, so the residual is
    // shared proportionally rather than piled onto one neighbour.
    expect(next.ratios[0]).toBeCloseTo(next.ratios[1])
    expect(next.ratios[0] + next.ratios[1] + next.ratios[2]).toBeCloseTo(1)
  })

  it("never forces an already-tiny absorbing pane to grow", () => {
    const tight: SplitNode = {
      type: "split",
      id: "s-root",
      orientation: "horizontal",
      children: [leaf("a"), leaf("b")],
      ratios: [0.95, 0.05],
    }
    const next = reanchorForContainerResize(
      tight,
      "horizontal",
      "end",
      300 / 200
    ) as SplitNode
    expect(next.ratios[1]).toBeCloseTo(0.05)
  })

  it("hands the same change to every child of a cross-axis stack", () => {
    // Stacked panes both span the column's full width, so both flank the
    // divider and the ratios (which run down the other axis) must not move.
    const stacked = splitGroup(leaf("a"), "a", "down", "b")
    expect(
      reanchorForContainerResize(stacked, "horizontal", "end", 300 / 240)
    ).toBe(stacked)
  })

  it("re-anchors through a nested stack on the moved edge", () => {
    // horizontal [ a | vertical [b, c] ] — b and c both touch the right edge.
    const nested = splitGroup(
      splitGroup(leaf("a"), "a", "right", "b"),
      "b",
      "down",
      "c"
    )
    const next = reanchorForContainerResize(
      nested,
      "horizontal",
      "end",
      300 / 240
    )
    const widths = paneWidths(next, 240)
    expect(widths.a).toBeCloseTo(150)
    expect(widths.b).toBeCloseTo(90)
    expect(widths.c).toBeCloseTo(90)
    const { groups } = computeRects(next)
    expect(groups.get("b")?.h).toBeCloseTo(50)
    expect(groups.get("c")?.h).toBeCloseTo(50)
  })

  it("returns the same tree for a no-op or degenerate scale", () => {
    const three = threeEqualPanes()
    const at = (scale: number) =>
      reanchorForContainerResize(three, "horizontal", "end", scale)
    expect(at(1)).toBe(three)
    expect(at(0)).toBe(three)
    expect(at(Number.NaN)).toBe(three)
    const single = leaf("a")
    expect(
      reanchorForContainerResize(single, "horizontal", "end", 300 / 240)
    ).toBe(single)
  })
})

describe("computeRects", () => {
  it("lays out a binary horizontal split with one divider", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const { groups, handles } = computeRects(two)
    expect(groups.get("a")).toEqual({ x: 0, y: 0, w: 50, h: 100 })
    expect(groups.get("b")).toEqual({ x: 50, y: 0, w: 50, h: 100 })
    expect(handles).toEqual([
      {
        splitId: "s-b",
        index: 0,
        orientation: "horizontal",
        x: 50,
        y: 0,
        length: 100,
        nodeStart: 0,
        nodeExtent: 100,
      },
    ])
  })

  it("computes nested rects and per-node handle geometry", () => {
    // horizontal [ vertical [a, c], b ]
    const two = splitGroup(leaf("a"), "a", "right", "b")
    const nested = splitGroup(two, "a", "down", "c")
    const { groups, handles } = computeRects(nested)
    expect(groups.get("a")).toEqual({ x: 0, y: 0, w: 50, h: 50 })
    expect(groups.get("c")).toEqual({ x: 0, y: 50, w: 50, h: 50 })
    expect(groups.get("b")).toEqual({ x: 50, y: 0, w: 50, h: 100 })
    const vertical = handles.find((h) => h.orientation === "vertical")
    expect(vertical).toMatchObject({
      splitId: "s-c",
      x: 0,
      y: 50,
      length: 50,
      nodeStart: 0,
      nodeExtent: 100,
    })
  })

  it("covers a single leaf with the full container and no handles", () => {
    const { groups, handles } = computeRects(leaf("a"))
    expect(groups.get("a")).toEqual({ x: 0, y: 0, w: 100, h: 100 })
    expect(handles).toEqual([])
  })
})

describe("isLayoutNode", () => {
  it("accepts valid trees and rejects malformed ones", () => {
    const two = splitGroup(leaf("a"), "a", "right", "b")
    expect(isLayoutNode(two)).toBe(true)
    expect(isLayoutNode(singleGroupLayout())).toBe(true)
    expect(isLayoutNode(null)).toBe(false)
    expect(isLayoutNode({ type: "group" })).toBe(false)
    expect(
      isLayoutNode({
        type: "split",
        id: "s",
        orientation: "horizontal",
        children: [leaf("a")],
        ratios: [1],
      })
    ).toBe(false)
    expect(
      isLayoutNode({
        type: "split",
        id: "s",
        orientation: "diagonal",
        children: [leaf("a"), leaf("b")],
        ratios: [0.5, 0.5],
      })
    ).toBe(false)
  })
})

describe("degenerate geometry hardening", () => {
  // A NaN ratio is worse than it looks: JSON.stringify writes it as `null`,
  // the persisted blob then fails validation on the next launch, and the user
  // silently loses their entire split layout.
  it("ignores a non-finite resize instead of poisoning the ratios", () => {
    const tree = splitGroup(leaf("a"), "a", "right", "b")
    for (const bad of [NaN, Infinity, -Infinity]) {
      const next = resizeSplitAt(tree, `s-b`, 0, bad)
      expect(next).toBe(tree)
    }
    const split = resizeSplitAt(tree, "s-b", 0, 0.3) as SplitNode
    expect(split.ratios.every((r) => Number.isFinite(r))).toBe(true)
    expect(JSON.parse(JSON.stringify(split)).ratios).toEqual(split.ratios)
  })

  it("rejects persisted ratios that are zero, negative, or non-finite", () => {
    const withRatios = (ratios: unknown[]) => ({
      type: "split",
      id: "s",
      orientation: "horizontal",
      children: [leaf("a"), leaf("b")],
      ratios,
    })
    expect(isLayoutNode(withRatios([0.5, 0.5]))).toBe(true)
    expect(isLayoutNode(withRatios([0, 1]))).toBe(false)
    expect(isLayoutNode(withRatios([-0.2, 1.2]))).toBe(false)
    expect(isLayoutNode(withRatios([null, 1]))).toBe(false)
    expect(isLayoutNode(withRatios([0.5]))).toBe(false)
  })

  it("normalizes ratios that do not sum to 1 so no group paints at zero size", () => {
    const skewed: LayoutNode = {
      type: "split",
      id: "s",
      orientation: "horizontal",
      children: [leaf("a"), leaf("b")],
      ratios: [0.1, 0.1],
    }
    const { groups } = computeRects(skewed)
    expect(groups.get("a")).toEqual({ x: 0, y: 0, w: 50, h: 100 })
    expect(groups.get("b")).toEqual({ x: 50, y: 0, w: 50, h: 100 })

    const broken: LayoutNode = {
      type: "split",
      id: "s",
      orientation: "vertical",
      children: [leaf("a"), leaf("b")],
      ratios: [0, 0],
    }
    const fallback = computeRects(broken).groups
    expect(fallback.get("a")).toEqual({ x: 0, y: 0, w: 100, h: 50 })
    expect(fallback.get("b")).toEqual({ x: 0, y: 50, w: 100, h: 50 })
    for (const rect of fallback.values()) {
      expect(Number.isFinite(rect.w) && rect.w > 0).toBe(true)
      expect(Number.isFinite(rect.h) && rect.h > 0).toBe(true)
    }
  })
})
