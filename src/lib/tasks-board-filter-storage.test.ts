import { beforeEach, describe, expect, it } from "vitest"
import {
  loadTasksBoardFilter,
  loadTasksBoardGrouping,
  loadTasksStatusFilter,
  loadTasksViewMode,
  saveTasksBoardFilter,
  saveTasksBoardGrouping,
  saveTasksStatusFilter,
  saveTasksViewMode,
} from "./tasks-board-filter-storage"

const BOARD_FILTER_KEY = "workspace:tasks-board-filter"
const VIEW_MODE_KEY = "workspace:tasks-view-mode"
const STATUS_FILTER_KEY = "workspace:tasks-status-filter"
const GROUPING_KEY = "workspace:tasks-board-grouping"

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
