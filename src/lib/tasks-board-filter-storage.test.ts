import { beforeEach, describe, expect, it } from "vitest"
import {
  loadTasksBoardFilter,
  loadTasksBoardGrouping,
  loadTasksOwnerFilter,
  loadTasksPriorityFilter,
  loadTasksScope,
  loadTasksSort,
  loadTasksStatusFilter,
  loadTasksViewMode,
  saveTasksBoardFilter,
  saveTasksBoardGrouping,
  saveTasksOwnerFilter,
  saveTasksPriorityFilter,
  saveTasksScope,
  saveTasksSort,
  saveTasksStatusFilter,
  saveTasksViewMode,
} from "./tasks-board-filter-storage"

const BOARD_FILTER_KEY = "workspace:tasks-board-filter"
const VIEW_MODE_KEY = "workspace:tasks-view-mode"
const STATUS_FILTER_KEY = "workspace:tasks-status-filter"
const GROUPING_KEY = "workspace:tasks-board-grouping"
const SCOPE_FILTER_KEY = "workspace:tasks-scope-filter"
const OWNER_FILTER_KEY = "workspace:tasks-owner-filter"
const PRIORITY_FILTER_KEY = "workspace:tasks-priority-filter"
const SORT_KEY = "workspace:tasks-sort"

beforeEach(() => {
  localStorage.clear()
})

describe("tasks board filter storage", () => {
  it("round-trips hidden columns and archive visibility", () => {
    expect(loadTasksBoardFilter()).toEqual({
      hiddenColumns: [],
      showArchived: false,
    })
    saveTasksBoardFilter({
      hiddenColumns: ["review", "canceled"],
      showArchived: true,
    })
    expect(loadTasksBoardFilter()).toEqual({
      hiddenColumns: ["review", "canceled"],
      showArchived: true,
    })
  })

  it("migrates the old showCanceled toggle to the hidden-column model", () => {
    localStorage.setItem(
      BOARD_FILTER_KEY,
      JSON.stringify({ showCanceled: false, showArchived: false })
    )
    expect(loadTasksBoardFilter()).toEqual({
      hiddenColumns: ["canceled"],
      showArchived: false,
    })
  })
})

describe("tasks view mode storage", () => {
  it("round-trips a stored mode and defaults to the board", () => {
    expect(loadTasksViewMode()).toBe("board")
    saveTasksViewMode("list")
    expect(loadTasksViewMode()).toBe("list")
    saveTasksViewMode("board")
    expect(loadTasksViewMode()).toBe("board")
  })

  it("falls back to the board on a junk entry", () => {
    localStorage.setItem(VIEW_MODE_KEY, "gallery")
    expect(loadTasksViewMode()).toBe("board")
  })
})

describe("tasks status filter storage", () => {
  it("round-trips a group and keeps null meaning every status", () => {
    expect(loadTasksStatusFilter()).toBeNull()
    saveTasksStatusFilter("review")
    expect(loadTasksStatusFilter()).toBe("review")
    // Clearing removes the entry rather than storing something that would read
    // back as a filter nobody chose.
    saveTasksStatusFilter(null)
    expect(localStorage.getItem(STATUS_FILTER_KEY)).toBeNull()
    expect(loadTasksStatusFilter()).toBeNull()
  })

  it("falls back to every status on an unknown entry — including the old per-status selection", () => {
    // What this key held while the filter was a checkbox menu. It matches no
    // group, so it degrades to "every status" and the next save overwrites it.
    localStorage.setItem(
      STATUS_FILTER_KEY,
      JSON.stringify(["review", "failed"])
    )
    expect(loadTasksStatusFilter()).toBeNull()

    localStorage.setItem(STATUS_FILTER_KEY, "awaiting_input")
    expect(loadTasksStatusFilter()).toBeNull()

    localStorage.setItem(STATUS_FILTER_KEY, "{not json")
    expect(loadTasksStatusFilter()).toBeNull()
  })
})

describe("tasks board grouping storage", () => {
  it("round-trips a stored grouping and defaults to none", () => {
    expect(loadTasksBoardGrouping()).toBe("none")
    saveTasksBoardGrouping("folder")
    expect(loadTasksBoardGrouping()).toBe("folder")
    saveTasksBoardGrouping("session")
    expect(loadTasksBoardGrouping()).toBe("session")
    saveTasksBoardGrouping("agent")
    expect(loadTasksBoardGrouping()).toBe("agent")
    saveTasksBoardGrouping("none")
    expect(loadTasksBoardGrouping()).toBe("none")
  })

  it("falls back to none on a junk entry", () => {
    localStorage.setItem(GROUPING_KEY, "collection")
    expect(loadTasksBoardGrouping()).toBe("none")
    localStorage.setItem(GROUPING_KEY, "room")
    expect(loadTasksBoardGrouping()).toBe("none")
  })
})

describe("tasks quick scope storage", () => {
  it("round-trips a scope and defaults to all", () => {
    expect(loadTasksScope()).toBe("all")
    saveTasksScope("agent")
    expect(loadTasksScope()).toBe("agent")
    saveTasksScope("attention")
    expect(loadTasksScope()).toBe("attention")
  })

  it("migrates the retired unassigned/manual tabs to all", () => {
    localStorage.setItem(SCOPE_FILTER_KEY, "unassigned")
    expect(loadTasksScope()).toBe("all")
    localStorage.setItem(SCOPE_FILTER_KEY, "manual")
    expect(loadTasksScope()).toBe("all")
  })

  it("falls back to all on a stale scope", () => {
    localStorage.setItem(SCOPE_FILTER_KEY, "members")
    expect(loadTasksScope()).toBe("all")
  })
})

describe("tasks owner filter storage", () => {
  it("round-trips a stable Session id and clears it", () => {
    expect(loadTasksOwnerFilter()).toBeNull()
    saveTasksOwnerFilter(42)
    expect(loadTasksOwnerFilter()).toBe(42)
    saveTasksOwnerFilter(null)
    expect(localStorage.getItem(OWNER_FILTER_KEY)).toBeNull()
  })

  it("ignores invalid ids", () => {
    localStorage.setItem(OWNER_FILTER_KEY, "0")
    expect(loadTasksOwnerFilter()).toBeNull()
    localStorage.setItem(OWNER_FILTER_KEY, "not-a-session")
    expect(loadTasksOwnerFilter()).toBeNull()
  })
})

describe("tasks priority filter storage", () => {
  it("round-trips a priority and clears back to all", () => {
    expect(loadTasksPriorityFilter()).toBeNull()
    saveTasksPriorityFilter("urgent")
    expect(loadTasksPriorityFilter()).toBe("urgent")
    saveTasksPriorityFilter("none")
    expect(loadTasksPriorityFilter()).toBe("none")
    saveTasksPriorityFilter(null)
    expect(localStorage.getItem(PRIORITY_FILTER_KEY)).toBeNull()
  })

  it("ignores stale priority values", () => {
    localStorage.setItem(PRIORITY_FILTER_KEY, "critical")
    expect(loadTasksPriorityFilter()).toBeNull()
  })
})

describe("tasks sort storage", () => {
  it("defaults to manual and round-trips the two projected sorts", () => {
    expect(loadTasksSort()).toBe("manual")
    saveTasksSort("priority")
    expect(loadTasksSort()).toBe("priority")
    saveTasksSort("updated")
    expect(loadTasksSort()).toBe("updated")
  })

  it("falls back to manual on stale values", () => {
    localStorage.setItem(SORT_KEY, "deadline")
    expect(loadTasksSort()).toBe("manual")
  })
})
