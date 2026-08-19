import { beforeEach, describe, expect, it } from "vitest"

import {
  DEFAULT_SECTION_ORDER,
  loadOrganizationMode,
  loadSectionCollapsed,
  loadSectionOrder,
  loadShowAgentCreated,
  loadShowAutomationCreated,
  loadShowRecent,
  moveSectionInOrder,
  normalizeSectionOrder,
  saveSectionOrder,
  saveOrganizationMode,
  saveShowAgentCreated,
  saveShowAutomationCreated,
  saveShowRecent,
} from "./sidebar-view-mode-storage"

const SECTION_ORDER_KEY = "workspace:sidebar-section-order"
const SHOW_RECENT_KEY = "workspace:sidebar-show-recent"
const SHOW_AGENT_CREATED_KEY = "workspace:sidebar-show-agent-created"
const SHOW_AUTOMATION_CREATED_KEY = "workspace:sidebar-show-automation-created"

describe("normalizeSectionOrder", () => {
  it("passes a complete order through unchanged", () => {
    expect(normalizeSectionOrder(["recent", "chats", "folders"])).toEqual([
      "recent",
      "chats",
      "folders",
    ])
  })

  it("appends missing sections in default order", () => {
    // Forward compatibility in reverse: a store written before "Recent" existed
    // gets it at the bottom rather than losing the section entirely.
    expect(normalizeSectionOrder(["chats", "folders"])).toEqual([
      "chats",
      "folders",
      "recent",
    ])
  })

  it("drops unknown entries and repeats", () => {
    expect(
      normalizeSectionOrder(["recent", "bogus", "recent", 7, null, "chats"])
    ).toEqual(["recent", "chats", "folders"])
  })

  it("migrates the legacy two-way strings, preserving the user's choice", () => {
    expect(normalizeSectionOrder("chats-first")).toEqual([
      "chats",
      "folders",
      "recent",
    ])
    expect(normalizeSectionOrder("folders-first")).toEqual(
      DEFAULT_SECTION_ORDER
    )
  })

  it("falls back to the default for anything unusable", () => {
    expect(normalizeSectionOrder(undefined)).toEqual(DEFAULT_SECTION_ORDER)
    expect(normalizeSectionOrder("nonsense")).toEqual(DEFAULT_SECTION_ORDER)
    expect(normalizeSectionOrder({ folders: 1 })).toEqual(DEFAULT_SECTION_ORDER)
    expect(normalizeSectionOrder([])).toEqual(DEFAULT_SECTION_ORDER)
  })
})

describe("moveSectionInOrder", () => {
  const order = ["folders", "chats", "recent"] as const

  it("moves a section one slot in either direction", () => {
    expect(moveSectionInOrder(order, "recent", -1)).toEqual([
      "folders",
      "recent",
      "chats",
    ])
    expect(moveSectionInOrder(order, "folders", 1)).toEqual([
      "chats",
      "folders",
      "recent",
    ])
  })

  it("moves across multiple slots", () => {
    expect(moveSectionInOrder(order, "recent", -2)).toEqual([
      "recent",
      "folders",
      "chats",
    ])
  })

  it("returns the SAME reference for a move that would fall off an end", () => {
    // Identity matters: the sidebar skips the state update and the localStorage
    // write when a clamped nudge changes nothing.
    expect(moveSectionInOrder(order, "folders", -1)).toBe(order)
    expect(moveSectionInOrder(order, "recent", 1)).toBe(order)
    expect(moveSectionInOrder(order, "chats", 0)).toBe(order)
  })
})

describe("section-order persistence", () => {
  beforeEach(() => localStorage.clear())

  it("round-trips through localStorage", () => {
    saveSectionOrder(["recent", "folders", "chats"])
    expect(loadSectionOrder()).toEqual(["recent", "folders", "chats"])
  })

  it("defaults to Folders → Chat → Recent with nothing stored", () => {
    expect(loadSectionOrder()).toEqual(["folders", "chats", "recent"])
  })

  it("migrates a legacy bare string left by an older build", () => {
    // The old format was not JSON, so the loader must survive the parse failure
    // rather than resetting the user's preference.
    localStorage.setItem(SECTION_ORDER_KEY, "chats-first")
    expect(loadSectionOrder()).toEqual(["chats", "folders", "recent"])
  })

  it("falls back to the default for corrupt JSON", () => {
    localStorage.setItem(SECTION_ORDER_KEY, "{oops")
    expect(loadSectionOrder()).toEqual(DEFAULT_SECTION_ORDER)
  })
})

describe("loadShowRecent", () => {
  beforeEach(() => localStorage.clear())

  it("defaults to on", () => {
    expect(loadShowRecent()).toBe(true)
  })

  it("respects an explicitly-stored false", () => {
    saveShowRecent(false)
    expect(localStorage.getItem(SHOW_RECENT_KEY)).toBe("false")
    expect(loadShowRecent()).toBe(false)
    saveShowRecent(true)
    expect(loadShowRecent()).toBe(true)
  })
})

describe("session-source switches", () => {
  beforeEach(() => localStorage.clear())

  it("default to on, so an untouched sidebar lists every source", () => {
    expect(loadShowAgentCreated()).toBe(true)
    expect(loadShowAutomationCreated()).toBe(true)
  })

  it("respect an explicitly-stored false", () => {
    saveShowAgentCreated(false)
    expect(localStorage.getItem(SHOW_AGENT_CREATED_KEY)).toBe("false")
    expect(loadShowAgentCreated()).toBe(false)
    saveShowAgentCreated(true)
    expect(loadShowAgentCreated()).toBe(true)

    saveShowAutomationCreated(false)
    expect(localStorage.getItem(SHOW_AUTOMATION_CREATED_KEY)).toBe("false")
    expect(loadShowAutomationCreated()).toBe(false)
  })

  it("are independent of each other", () => {
    // Keeping scheduled runs while dropping delegation children (or the
    // reverse) is the whole point of two switches instead of one.
    saveShowAgentCreated(false)
    expect(loadShowAutomationCreated()).toBe(true)
  })
})

describe("organization mode", () => {
  beforeEach(() => localStorage.clear())

  it("defaults to semantic Collections and persists execution locations", () => {
    expect(loadOrganizationMode()).toBe("collections")
    saveOrganizationMode("locations")
    expect(loadOrganizationMode()).toBe("locations")
  })

  it("ignores an unknown stored mode", () => {
    localStorage.setItem("workspace:sidebar-organization-mode", "folders-v0")
    expect(loadOrganizationMode()).toBe("collections")
  })
})

describe("loadSectionCollapsed", () => {
  beforeEach(() => localStorage.clear())

  it("reads the recent section's collapsed flag", () => {
    localStorage.setItem(
      "workspace:sidebar-section-collapsed",
      JSON.stringify({ recent: true, chats: false, bogus: 1 })
    )
    expect(loadSectionCollapsed()).toEqual({ recent: true, chats: false })
  })
})
