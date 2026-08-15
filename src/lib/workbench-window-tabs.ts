"use client"

export const WORKBENCH_WINDOW_TABS_STORAGE_KEY =
  "workspace:window-workbench-tabs:v1"
export const MAX_RECENTLY_CLOSED_WORKBENCHES = 10

export interface WorkbenchWindowTabsState {
  openIds: number[]
  recentlyClosedIds: number[]
}

function uniqueValidIds(
  value: unknown,
  validIds: ReadonlySet<number>
): number[] {
  if (!Array.isArray(value)) return []
  const seen = new Set<number>()
  const result: number[] = []
  for (const item of value) {
    if (
      typeof item !== "number" ||
      !Number.isInteger(item) ||
      !validIds.has(item) ||
      seen.has(item)
    ) {
      continue
    }
    seen.add(item)
    result.push(item)
  }
  return result
}

/**
 * Restore the logical Workbench tabs mounted in this app window. Workbenches
 * remain durable backend entities; closing one here only removes a local view.
 * A missing preference preserves the pre-feature behaviour by opening every
 * saved Workbench once.
 */
export function loadWorkbenchWindowTabs(
  allWorkbenchIds: readonly number[],
  activeWorkbenchId: number
): WorkbenchWindowTabsState {
  const validIds = new Set(allWorkbenchIds)
  const fallback = validIds.has(activeWorkbenchId)
    ? activeWorkbenchId
    : allWorkbenchIds[0]
  if (typeof window === "undefined") {
    return {
      openIds: fallback == null ? [] : [fallback],
      recentlyClosedIds: [],
    }
  }

  try {
    const raw = localStorage.getItem(WORKBENCH_WINDOW_TABS_STORAGE_KEY)
    if (!raw) {
      return {
        openIds: [...allWorkbenchIds],
        recentlyClosedIds: [],
      }
    }
    const parsed = JSON.parse(raw) as {
      openIds?: unknown
      recentlyClosedIds?: unknown
    }
    const openIds = uniqueValidIds(parsed.openIds, validIds)
    const recentlyClosedIds = uniqueValidIds(
      parsed.recentlyClosedIds,
      validIds
    ).filter((id) => !openIds.includes(id))

    // The active surface must always have a visible tab. This also repairs a
    // stale preference after a backend restore or another client changed the
    // active Workbench.
    if (fallback != null && !openIds.includes(fallback)) openIds.push(fallback)
    return {
      openIds,
      recentlyClosedIds: recentlyClosedIds.slice(
        0,
        MAX_RECENTLY_CLOSED_WORKBENCHES
      ),
    }
  } catch {
    return {
      openIds: [...allWorkbenchIds],
      recentlyClosedIds: [],
    }
  }
}

export function saveWorkbenchWindowTabs(state: WorkbenchWindowTabsState): void {
  if (typeof window === "undefined") return
  try {
    localStorage.setItem(
      WORKBENCH_WINDOW_TABS_STORAGE_KEY,
      JSON.stringify(state)
    )
  } catch {
    /* localStorage is an optional convenience, never a reason to block work. */
  }
}

export function openWorkbenchWindowTab(
  state: WorkbenchWindowTabsState,
  id: number
): WorkbenchWindowTabsState {
  return {
    openIds: state.openIds.includes(id)
      ? state.openIds
      : [...state.openIds, id],
    recentlyClosedIds: state.recentlyClosedIds.filter(
      (candidate) => candidate !== id
    ),
  }
}

export function closeWorkbenchWindowTab(
  state: WorkbenchWindowTabsState,
  id: number
): WorkbenchWindowTabsState {
  if (!state.openIds.includes(id) || state.openIds.length <= 1) return state
  return {
    openIds: state.openIds.filter((candidate) => candidate !== id),
    recentlyClosedIds: [
      id,
      ...state.recentlyClosedIds.filter((candidate) => candidate !== id),
    ].slice(0, MAX_RECENTLY_CLOSED_WORKBENCHES),
  }
}
