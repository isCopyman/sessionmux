"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { Reorder } from "motion/react"
import type { PanInfo } from "motion/react"
import { SquarePen } from "lucide-react"
import { useTranslations } from "next-intl"
import { cn } from "@/lib/utils"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useActiveFolder } from "@/contexts/active-folder-context"
import { useTabActions, useTabStore } from "@/contexts/tab-context"
import type { TabItem as TabItemData } from "@/contexts/tab-context"
import { groupOfTab } from "@/stores/tab-store"
import {
  firstLeafId,
  leafIds,
  type SplitDirection,
} from "@/lib/tab-group-layout"
import {
  clientPointFromDrag,
  dropIndexFromMidpoints,
  moveIdToDropIndex,
  splitDropEdgeFromPoint,
  type DragClientPoint,
  type SplitDropEdge,
} from "@/lib/tab-drag-drop"
import { useWorkbenchRoute } from "@/contexts/workbench-route-context"
import { useIsCoarsePointer } from "@/hooks/use-is-coarse-pointer"
import { TabItem, type TabMoveTarget } from "./tab-item"

interface TabBarProps {
  /** Split-group strip: render only this group's tabs, highlight the GROUP's
   *  selected tab, and target new tabs/reorders at the group. Omitted = the
   *  single title-bar strip shown while unsplit. */
  groupId?: string
}

interface PaneDropTarget {
  gid: string
  el: Element
  strip: boolean
  splitEdge: SplitDropEdge | null
  /** The source pane's center is still a visible Paseo-style target, but
   * releasing there must not reorder, move, or split the tab. */
  noOp: boolean
}

// Rendered inside the desktop conversation-column title strip while unsplit,
// or once per group shell (with `groupId`) while split. The old standalone
// mobile variant is gone — mobile shows the conversation detail header instead
// and navigates tabs from the sidebar.
export function TabBar({ groupId }: TabBarProps) {
  const t = useTranslations("Folder.conversationCard")
  const tabs = useTabStore((s) => s.tabs)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const groupOf = useTabStore((s) => s.groupOf)
  const groupLayout = useTabStore((s) => s.groupLayout)
  const groupSelection = useTabStore((s) => s.groupSelection)
  const tileByGroup = useTabStore((s) => s.tileByGroup)
  const {
    switchTab,
    closeTab,
    closeOtherTabs,
    closeAllTabs,
    pinTab,
    toggleGroupTile,
    splitTab,
    snapTabToSplit,
    moveTabToGroup,
    toggleGroupOrientation,
    dissolveGroup,
    unsplitAll,
    reorderTabs,
    reorderGroupTabs,
    updateTabDrag,
    endTabDrag,
    openNewConversationTab,
    openChatModeTab,
  } = useTabActions()
  const allFolders = useAppWorkspaceStore((s) => s.allFolders)
  const branches = useAppWorkspaceStore((s) => s.branches)
  const { activeFolder } = useActiveFolder()
  const { openConversations } = useWorkbenchRoute()

  // The group this strip represents (unsplit strip = the single first leaf),
  // its tabs, and its displayed "active" tab: the GROUP's selected tab — for
  // the focused group (and the unsplit strip) that IS the global active tab.
  const stripGroupId = groupId ?? firstLeafId(groupLayout)
  const groupTabs = useMemo(
    () =>
      groupId == null
        ? tabs
        : tabs.filter(
            (tab) => groupOfTab(groupOf, groupLayout, tab.id) === groupId
          ),
    [tabs, groupId, groupOf, groupLayout]
  )
  // Unlike Motion's default live reorder, conversation tabs keep their source
  // slot while a floating duplicate moves. A source-strip drop computes and
  // commits the final order once; pane-body drops belong solely to layout.
  const displayedGroupTabs = groupTabs
  const displayActiveId =
    groupId == null ? activeTabId : (groupSelection[groupId] ?? null)
  const isTileMode = !!tileByGroup[stripGroupId]
  const handleToggleTile = useCallback(
    () => toggleGroupTile(stripGroupId),
    [toggleGroupTile, stripGroupId]
  )

  // Split-group context-menu wiring, shared by every tab in this strip.
  const orderedLeaves = useMemo(() => leafIds(groupLayout), [groupLayout])
  const isSplit = orderedLeaves.length > 1
  const moveTargets = useMemo<TabMoveTarget[]>(() => {
    if (!isSplit) return []
    return orderedLeaves
      .map((leafId, index) => ({
        groupId: leafId,
        index: index + 1,
        title:
          tabs.find((tab) => tab.id === groupSelection[leafId])?.title ?? null,
      }))
      .filter((target) => target.groupId !== stripGroupId)
  }, [isSplit, orderedLeaves, tabs, groupSelection, stripGroupId])
  const handleSplit = useCallback(
    (tabId: string, direction: SplitDirection, move: boolean) =>
      splitTab(tabId, direction, { move }),
    [splitTab]
  )
  const handleToggleSplitOrientation = useCallback(
    () => toggleGroupOrientation(stripGroupId),
    [toggleGroupOrientation, stripGroupId]
  )
  const handleUnsplit = useCallback(
    () => dissolveGroup(stripGroupId),
    [dissolveGroup, stripGroupId]
  )

  // ── Pane drag & drop + edge snapping ────────────────────────────────────
  // The dragged tab itself is axis-locked to its own strip (Reorder drag="x" +
  // overflow clipping), so crossing groups is pointer-based: hit-test the
  // pane shell under the cursor, highlight either its center or nearest edge,
  // and commit the move/split on release. The source pane's center remains an
  // ordinary within-strip reorder; its edges are valid split targets.
  const isDropTarget = useTabStore(
    (s) =>
      groupId != null &&
      s.tabDrag?.overGroupId === groupId &&
      s.tabDrag?.splitEdge == null
  )
  const resolveDropTarget = useCallback(
    (
      clientX: number,
      clientY: number,
      translatedCenter: DragClientPoint | null
    ): PaneDropTarget | null => {
      const el = document.elementFromPoint(clientX, clientY)
      if (!el) return null
      const strip = el.closest("[data-conv-group-strip]")
      if (strip) {
        const gid = strip.getAttribute("data-conv-group-strip")
        return gid
          ? {
              gid,
              el: strip,
              strip: true,
              splitEdge: null,
              noOp: false,
            }
          : null
      }
      const shell = el.closest("[data-conv-group-shell]")
      if (shell) {
        const gid = shell.getAttribute("data-conv-group-shell")
        if (!gid) return null
        const dragState = useTabStore.getState().tabDrag
        const splitEdge = splitDropEdgeFromPoint(
          translatedCenter?.x ?? clientX,
          translatedCenter?.y ?? clientY,
          shell.getBoundingClientRect(),
          {
            currentEdge:
              dragState?.overGroupId === gid
                ? (dragState.splitEdge ?? null)
                : null,
          }
        )
        return {
          gid,
          el: shell,
          strip: false,
          splitEdge,
          noOp: gid === stripGroupId && splitEdge == null,
        }
      }
      return null
    },
    [stripGroupId]
  )
  const handleTabDrag = useCallback(
    (
      tab: TabItemData,
      event: MouseEvent | TouchEvent | PointerEvent,
      info: PanInfo,
      translatedCenter: DragClientPoint | null
    ) => {
      const { x, y } = clientPointFromDrag(event, info)
      const target = resolveDropTarget(x, y, translatedCenter)
      updateTabDrag({
        tabId: tab.id,
        title: tab.title,
        x,
        y,
        overGroupId:
          target?.strip && target.gid === stripGroupId
            ? null
            : (target?.gid ?? null),
        splitEdge: target?.splitEdge ?? null,
      })
    },
    [resolveDropTarget, stripGroupId, updateTabDrag]
  )
  const handleTabDragStart = useCallback(
    (
      tab: TabItemData,
      event: MouseEvent | TouchEvent | PointerEvent,
      info: PanInfo,
      translatedCenter: DragClientPoint | null
    ) => {
      handleTabDrag(tab, event, info, translatedCenter)
    },
    [handleTabDrag]
  )
  const handleTabDragEnd = useCallback(
    (
      tab: TabItemData,
      event: MouseEvent | TouchEvent | PointerEvent,
      info: PanInfo,
      translatedCenter: DragClientPoint | null
    ) => {
      const { x, y } = clientPointFromDrag(event, info)
      const target = resolveDropTarget(x, y, translatedCenter)
      endTabDrag()
      if (!target) return
      if (target.strip && target.gid === stripGroupId) {
        const state = useTabStore.getState()
        const currentGroupTabs =
          groupId == null
            ? state.tabs
            : state.tabs.filter(
                (item) =>
                  groupOfTab(state.groupOf, state.groupLayout, item.id) ===
                  groupId
              )
        const dropIndex = dropIndexFromMidpoints(
          x,
          Array.from(target.el.querySelectorAll("[data-tab-id]")).map(
            (tabEl) => {
              const rect = tabEl.getBoundingClientRect()
              return rect.left + rect.width / 2
            }
          )
        )
        const orderedIds = moveIdToDropIndex(
          currentGroupTabs.map((item) => item.id),
          tab.id,
          dropIndex
        )
        const byId = new Map(currentGroupTabs.map((item) => [item.id, item]))
        const ordered = orderedIds.flatMap((id) => {
          const item = byId.get(id)
          return item ? [item] : []
        })
        if (groupId == null) reorderTabs(ordered)
        else reorderGroupTabs(groupId, ordered)
        return
      }

      if (target.noOp) return
      if (target.splitEdge) {
        snapTabToSplit(tab.id, target.gid, target.splitEdge)
        return
      }
      // Strip drop: land at the cursor position (midpoint count). Shell-body
      // drop: append (the store clamps the oversized index to the tail).
      const index = target.strip
        ? dropIndexFromMidpoints(
            x,
            Array.from(target.el.querySelectorAll("[data-tab-id]")).map(
              (tabEl) => {
                const rect = tabEl.getBoundingClientRect()
                return rect.left + rect.width / 2
              }
            )
          )
        : Number.MAX_SAFE_INTEGER
      moveTabToGroup(tab.id, target.gid, { index })
    },
    [
      resolveDropTarget,
      endTabDrag,
      groupId,
      moveTabToGroup,
      reorderGroupTabs,
      reorderTabs,
      snapTabToSplit,
      stripGroupId,
    ]
  )

  // New-conversation affordance at the end of the tab strip. Mirrors the
  // sidebar's "New chat": return to the conversation workspace, then open a
  // draft — or a folderless chat when no context resolves, so the button is
  // never a dead end. Group strips seed from the GROUP's own selection (its
  // folder / chat mode) rather than the globally-active folder: each group is
  // its own workspace slice, and the focused group may be a different one.
  const handleNewConversation = useCallback(() => {
    openConversations()
    const groupOptions = groupId != null ? { targetGroup: groupId } : undefined
    if (groupId != null) {
      const selTab =
        groupTabs.find((tab) => tab.id === displayActiveId) ?? groupTabs[0]
      const selFolder = selTab
        ? allFolders.find((f) => f.id === selTab.folderId)
        : undefined
      if (selTab?.isChat === true || selFolder?.kind === "chat") {
        openChatModeTab(groupOptions)
        return
      }
      if (selTab && selFolder) {
        openNewConversationTab(
          selFolder.id,
          selTab.workingDir ?? selFolder.path,
          groupOptions
        )
        return
      }
      // Group context unresolvable (folder deleted) — fall through to the
      // active-folder default.
    }
    if (!activeFolder) {
      openChatModeTab(groupOptions)
      return
    }
    openNewConversationTab(activeFolder.id, activeFolder.path, groupOptions)
  }, [
    activeFolder,
    allFolders,
    displayActiveId,
    groupId,
    groupTabs,
    openChatModeTab,
    openConversations,
    openNewConversationTab,
  ])

  const folderIndex = useMemo(() => {
    const map = new Map<number, { name: string }>()
    for (const f of allFolders) map.set(f.id, { name: f.name })
    return map
  }, [allFolders])

  const scrollRef = useRef<HTMLDivElement>(null)
  const isCoarsePointer = useIsCoarsePointer()
  const [touchSortingTabId, setTouchSortingTabId] = useState<string | null>(
    null
  )

  useEffect(() => {
    if (!displayActiveId || !scrollRef.current) return
    const el = scrollRef.current.querySelector(
      `[data-tab-id="${displayActiveId}"]`
    )
    el?.scrollIntoView({ block: "nearest", inline: "nearest" })
  }, [displayActiveId])

  // Reorder.Item still supplies robust pointer/touch drag controls, but its
  // repeated midpoint previews are deliberately ignored. See drag-end commit.
  const handleReorder = useCallback(() => {}, [])

  const handleTouchSortingEnd = useCallback(
    () => setTouchSortingTabId(null),
    []
  )

  if (groupTabs.length === 0) return null

  const activeIndex = displayedGroupTabs.findIndex(
    (tab) => tab.id === displayActiveId
  )
  // When the LAST tab is active, the trailing new-conversation wrapper is its
  // right neighbour — it needs the same baseline inset a tab neighbour gets
  // (`data-adjacent-active`), so the active tab's right reverse-corner foot
  // doesn't leave a stray line poking out from under it (globals.css).
  const lastTabActive = activeIndex >= 0 && activeIndex === groupTabs.length - 1

  return (
    <Reorder.Group
      as="div"
      ref={scrollRef}
      role="tablist"
      axis="x"
      values={displayedGroupTabs}
      onReorder={handleReorder}
      // Pane drop target: strips advertise their group id for center moves;
      // shells advertise the larger area and edge-split zones.
      data-conv-group-strip={stripGroupId}
      // Fills the title-bar strip and shrinks browser-style to share the row (see
      // TabItem): flush (`gap-0`) so hairline separators read as dividers, no
      // scrollbar (`overflow-hidden` still scrolls programmatically), and no
      // bottom border so the active (white) tab merges into the detail header
      // below. It hosts the trailing new-conversation button + drag spacer as its
      // own last children so the tabs, button, and spacer size in ONE flex line:
      // the tabs keep their equal `basis-48` width until the row fills, then
      // shrink together, and the button always hugs the last tab. `pl-2` only
      // (NOT `px-2`): the first tab keeps its left gutter for the first-child
      // seam-patch, but there's NO right padding so the trailing wrapper's
      // `ws-strip-line` reaches the group's right edge and the bottom hairline
      // stays continuous into the right reserve.
      className={cn(
        "pt-1.5 flex h-full min-w-0 flex-1 items-stretch gap-0 overflow-hidden pl-2",
        isDropTarget && "bg-primary/8"
      )}
    >
      {displayedGroupTabs.map((tab, index) => {
        const folderInfo = folderIndex.get(tab.folderId)
        // Neighbours of the active tab inset their workspace-bg baseline so the
        // active tab's transparent reverse-corner foot (which flares over them)
        // doesn't leave a stray line under it (globals.css `data-adjacent-active`).
        const adjacentActive =
          activeIndex < 0
            ? undefined
            : index === activeIndex - 1
              ? "before"
              : index === activeIndex + 1
                ? "after"
                : undefined
        return (
          <TabItem
            key={tab.id}
            tab={tab}
            isActive={tab.id === displayActiveId}
            isFocused={tab.id === activeTabId}
            isTileMode={isTileMode}
            embedded
            adjacentActive={adjacentActive}
            folderName={folderInfo?.name ?? null}
            folderBranch={branches.get(tab.folderId) ?? null}
            isSplit={isSplit}
            canSplitMove
            canMoveToGroup
            moveTargets={moveTargets}
            onTabDrag={handleTabDrag}
            onTabDragStart={handleTabDragStart}
            onTabDragEnd={handleTabDragEnd}
            onSwitch={switchTab}
            onClose={closeTab}
            onCloseOthers={closeOtherTabs}
            onCloseAll={closeAllTabs}
            onPin={pinTab}
            onToggleTile={handleToggleTile}
            onSplit={handleSplit}
            onMoveToGroup={moveTabToGroup}
            onToggleSplitOrientation={handleToggleSplitOrientation}
            onUnsplit={handleUnsplit}
            onUnsplitAll={unsplitAll}
            isCoarsePointer={isCoarsePointer}
            isTouchSorting={touchSortingTabId === tab.id}
            onTouchSortingStart={setTouchSortingTabId}
            onTouchSortingEnd={handleTouchSortingEnd}
          />
        )
      })}
      {/* The new-conversation button + drag spacer are the Reorder.Group's own
          trailing children, so they share the tabs' flex line — the button hugs
          the last tab and the spacer fills the leftover row as a window-drag
          region. They are not Reorder.Items, so dragging a tab only ever permutes
          the tabs. Wrapped in one `flex-1` `ws-strip-line` box so the
          workspace-bg bottom hairline runs unbroken under both — the short
          `self-start h-7` button can't carry the line itself. NO `min-w-0`: its
          min-content (the shrink-0 button + the spacer's `min-w-10`) is its floor,
          so under many-tab overflow the tabs shrink to reserve it instead of it
          collapsing to 0 and clipping the button. */}
      <div
        // `relative` anchors two decorative pseudo-elements: the
        // `data-adjacent-active` inset baseline (globals.css `.ws-strip-line`
        // `::after`) used when the last tab is active, and the `tab-strip-tail`
        // `::before` vertical separator shown between the last NON-active tab and
        // the new-conversation button. Inter-tab separators sit on each tab's
        // LEFT edge (`.browser-tab-item::before`), so the last tab's RIGHT edge —
        // where this flush-pinned button begins — otherwise has none. Only the
        // conversation strip carries `tab-strip-tail`; the file strip floats its
        // trailing button far-right past a drag spacer, so it stays divider-free.
        data-adjacent-active={lastTabActive ? "after" : undefined}
        className="tab-strip-tail relative flex h-full flex-1 items-stretch ws-strip-line"
      >
        <button
          type="button"
          onClick={handleNewConversation}
          // Ghost-style CIRCULAR icon button, evenly inset from the strip's three
          // visible edges so its round hover fill never touches the last tab.
          // `self-start` seats it against the group's `pt-1.5` top rather than
          // centering in the pt-shortened trailing box: with `h-7` on the `h-10`
          // strip that yields an equal 6px top and 6px bottom gap, so its center
          // still lands on the strip midline (matching the tab content). `ml-1.5`
          // adds a matching 6px LEFT gap from the last tab's edge. The hover uses
          // the chrome-standard adaptive tint (`bg-foreground/10`, matching the
          // bottom branch/command blocks) plus `backdrop-blur-sm`: over the fully
          // transparent strip (workspace bg image on) the fill reads as frosted
          // glass rather than a muddy patch, and the tint is clearly visible in
          // both light and dark themes (unlike the old near-white `bg-accent/40`).
          className="ml-1.5 mr-0.5 flex h-7 w-7 shrink-0 items-center justify-center self-start rounded-full text-muted-foreground backdrop-blur-sm transition-colors hover:bg-foreground/10 hover:text-foreground"
          aria-label={t("newConversation")}
          title={t("newConversation")}
        >
          <SquarePen className="h-3.5 w-3.5" />
        </button>
        {/* Drag spacer, floored at `min-w-10` (40px) instead of `min-w-0`: even
            when many tabs overflow and squeeze this region, a grabbable
            window-drag gap always remains to the RIGHT of the new-conversation
            button, so the button never reaches the strip's right edge and the
            packed strip stays draggable. Group strips keep the drag region too:
            while split there is NO dedicated title-bar row above the shells
            (the workspace layout drops it), so each strip's tail is that
            group's slice of the window-drag surface — for the top row it IS
            the title bar, and lower rows offer the same grab area, mirroring
            the unsplit strip. */}
        <div data-tauri-drag-region className="h-full min-w-10 flex-1" />
      </div>
    </Reorder.Group>
  )
}
