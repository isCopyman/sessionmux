import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import { describe, it, expect } from "vitest"

const tabBar = readFileSync(
  resolve(process.cwd(), "src/components/tabs/tab-bar.tsx"),
  "utf8"
)
const tabItem = readFileSync(
  resolve(process.cwd(), "src/components/tabs/tab-item.tsx"),
  "utf8"
)

/**
 * Wiring the store's guarantees cannot enforce on its own.
 *
 * Draft composer state is keyed by stable tab id, so a new-conversation tab can
 * use the same pane move/split affordances as a persisted Session.
 */
describe("tab strip draft gating", () => {
  it("offers every cross-group affordance to drafts", () => {
    expect(tabBar).not.toContain("const isDraft = tab.conversationId == null")
    expect(tabBar).toContain("canSplitMove")
    expect(tabBar).toContain("canMoveToGroup")
    expect(tabBar).toContain("onTabDrag={handleTabDrag}")
    expect(tabBar).toContain("onTabDragEnd={handleTabDragEnd}")
  })

  it("gates only the move items, so a draft keeps the group-management menu", () => {
    expect(tabItem).toContain("{canMoveToGroup &&")
    // `moveTargets` still drives the Unsplit All gate — passing an empty array
    // for drafts (instead of this flag) would have hidden that item too.
    expect(tabItem).toContain("{moveTargets.length >= 2 && (")
  })
})

describe("tab reorder transaction wiring", () => {
  it("keeps midpoint previews local and commits only on drag end", () => {
    expect(tabBar).toContain("const [previewOrderIds, setPreviewOrderIds]")
    expect(tabBar).toContain("previewOrderIdsRef.current = nextIds")
    expect(tabBar).toMatch(
      /if \(groupId == null\) reorderTabs\(ordered\)[\s\S]{0,80}else reorderGroupTabs\(groupId, ordered\)/
    )
  })
})

describe("tab drag selection guard wiring", () => {
  it("suppresses text selection for EVERY tab drag, composed with the long-press handlers", () => {
    // Held on drag start / released on drag end + unmount, so within-group
    // sorting and the unsplit strip are covered too (the ghost only exists for
    // cross-group drags).
    expect(tabItem).toContain("acquireDragSelectionGuard")
    expect(tabItem).toContain("releaseDragSelectionGuard")
    expect(tabItem).toContain("useEffect(() => releaseGuard, [releaseGuard])")
    // The long-press hook ships its own onDragStart/onDragEnd (coarse-pointer
    // cleanup + post-drag click suppression); ours must call through, never
    // replace them via spread order.
    expect(tabItem).toContain("onDragStart: longPressDragStart")
    expect(tabItem).toContain("onDragEnd: longPressDragEnd")
    expect(tabItem).toMatch(
      /handleDragStart[\s\S]{0,160}longPressDragStart\(\)/
    )
    expect(tabItem).toMatch(/handleDragEnd[\s\S]{0,200}longPressDragEnd\(\)/)
  })
})
