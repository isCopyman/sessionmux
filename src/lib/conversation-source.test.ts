import { describe, expect, it } from "vitest"

import {
  ALL_SESSION_SOURCES_VISIBLE,
  conversationSource,
  isSessionSourceVisible,
  matchesSessionSource,
} from "./conversation-source"
import type { ConversationCreatedBy } from "./types"

/** Only `created_by` matters here; the cast keeps fixtures to the one field. */
function row(created_by?: ConversationCreatedBy) {
  return { created_by }
}

describe("conversationSource", () => {
  it("returns the recorded source", () => {
    expect(conversationSource(row("user"))).toBe("user")
    expect(conversationSource(row("agent"))).toBe("agent")
    expect(conversationSource(row("automation"))).toBe("automation")
  })

  it("reads a missing source as user-created", () => {
    // Rows written before the column existed, and payloads from a server that
    // doesn't send the field yet.
    expect(conversationSource(row())).toBe("user")
    expect(conversationSource({ created_by: undefined })).toBe("user")
  })

  it("reads an unknown source as user-created rather than dropping it", () => {
    // A newer server sending a source this build has no UI for must not make
    // the Session invisible to every facet.
    const futureSource = "webhook" as unknown as ConversationCreatedBy
    expect(conversationSource({ created_by: futureSource })).toBe("user")
  })
})

describe("matchesSessionSource", () => {
  it("passes everything through on 'all'", () => {
    expect(matchesSessionSource(row("agent"), "all")).toBe(true)
    expect(matchesSessionSource(row(), "all")).toBe(true)
  })

  it("narrows to one source", () => {
    expect(matchesSessionSource(row("agent"), "agent")).toBe(true)
    expect(matchesSessionSource(row("agent"), "user")).toBe(false)
    expect(matchesSessionSource(row("automation"), "automation")).toBe(true)
  })

  it("counts a source-less row as user-created", () => {
    expect(matchesSessionSource(row(), "user")).toBe(true)
    expect(matchesSessionSource(row(), "agent")).toBe(false)
  })
})

describe("isSessionSourceVisible", () => {
  it("shows every source with both switches on", () => {
    for (const source of ["user", "agent", "automation"] as const) {
      expect(
        isSessionSourceVisible(row(source), ALL_SESSION_SOURCES_VISIBLE)
      ).toBe(true)
    }
  })

  it("hides only the switched-off source", () => {
    const noAgents = { showAgentCreated: false, showAutomationCreated: true }
    expect(isSessionSourceVisible(row("agent"), noAgents)).toBe(false)
    expect(isSessionSourceVisible(row("automation"), noAgents)).toBe(true)
    expect(isSessionSourceVisible(row("user"), noAgents)).toBe(true)
  })

  it("keeps user-created Sessions visible with both switches off", () => {
    // "Only the ones I started" is the floor of this facet — there is no switch
    // that can empty the list.
    const neither = { showAgentCreated: false, showAutomationCreated: false }
    expect(isSessionSourceVisible(row("user"), neither)).toBe(true)
    expect(isSessionSourceVisible(row(), neither)).toBe(true)
    expect(isSessionSourceVisible(row("agent"), neither)).toBe(false)
    expect(isSessionSourceVisible(row("automation"), neither)).toBe(false)
  })
})
