import { describe, expect, it } from "vitest"

import type { SuggestionGroup } from "@/components/chat/composer/suggestion/types"

import {
  buildRoomMentionSearch,
  buildRoomSessionGroup,
  combineRoomMentionGroups,
  type RoomMentionLabels,
  type RoomSessionMember,
} from "./room-mention-search"

const members: RoomSessionMember[] = [
  { conversationId: 3, title: "Session C", agentType: "codex" },
  { conversationId: 4, title: "Session D", agentType: "claude_code" },
]

const labels: RoomMentionLabels = {
  sessionGroupLabel: "Sessions",
  allLabel: "@all",
  humanLabel: "@human",
  untitled: (id) => `Untitled #${id}`,
}

function fileGroup(items: SuggestionGroup["items"] = []): SuggestionGroup {
  return { kind: "file", label: "Files", items }
}
function commitGroup(items: SuggestionGroup["items"] = []): SuggestionGroup {
  return { kind: "commit", label: "Commits", items }
}
function agentGroup(items: SuggestionGroup["items"] = []): SuggestionGroup {
  return { kind: "agent", label: "Agents", items }
}
// The generic, workspace-wide session group `useReferenceSearch` would
// otherwise return — must never leak into a room's panel.
function workspaceSessionGroup(): SuggestionGroup {
  return {
    kind: "session",
    label: "Sessions",
    items: [
      {
        reference: {
          refType: "session",
          id: "999",
          label: "Some other conversation",
          uri: "codeg://session/999",
          meta: null,
        },
      },
    ],
  }
}

const fileItem = {
  reference: {
    refType: "file" as const,
    id: "src/foo.ts",
    label: "foo.ts",
    uri: "file:///repo/src/foo.ts",
    meta: null,
  },
}

const agentItem = {
  reference: {
    refType: "agent" as const,
    id: "codex",
    label: "Codex",
    uri: null,
    meta: null,
  },
}

describe("buildRoomSessionGroup", () => {
  it("always leads with @all / @human plus every member", () => {
    const group = buildRoomSessionGroup("", members, labels)
    expect(group.kind).toBe("session")
    expect(group.items.map((item) => item.reference.label)).toEqual([
      "@all",
      "@human",
      "Session C",
      "Session D",
    ])
  })

  it("ranks members against the query, keeping @all / @human available", () => {
    const group = buildRoomSessionGroup("session d", members, labels)
    expect(group.items[0].reference.label).toBe("Session D")
  })

  it("falls back to the untitled formatter for a member with no title", () => {
    const group = buildRoomSessionGroup(
      "",
      [{ conversationId: 9, title: null, agentType: null }],
      labels
    )
    expect(
      group.items.find((item) => item.reference.id === "9")?.reference.label
    ).toBe("Untitled #9")
  })
})

describe("combineRoomMentionGroups", () => {
  it("keeps the session group first and folds in file + commit, dropping agent and the search's own session group", () => {
    const session = buildRoomSessionGroup("", members, labels)
    const combined = combineRoomMentionGroups(session, [
      fileGroup([fileItem]),
      agentGroup([agentItem]),
      workspaceSessionGroup(),
      commitGroup(),
    ])
    expect(combined.map((group) => group.kind)).toEqual([
      "session",
      "file",
      "commit",
    ])
    expect(combined[0]).toBe(session)
    expect(combined[1].items).toEqual([fileItem])
  })

  it("keeps an empty file/commit group (not missing) when the search found nothing", () => {
    const session = buildRoomSessionGroup("", members, labels)
    const combined = combineRoomMentionGroups(session, [
      fileGroup([]),
      commitGroup([]),
    ])
    const file = combined.find((group) => group.kind === "file")
    expect(file).toBeDefined()
    expect(file?.items).toEqual([])
    // The session group is unaffected by an empty workspace search.
    expect(combined[0].items.length).toBeGreaterThan(0)
  })

  it("omits the file group entirely when the search result doesn't include one", () => {
    const session = buildRoomSessionGroup("", members, labels)
    const combined = combineRoomMentionGroups(session, [])
    expect(combined).toEqual([session])
  })
})

describe("buildRoomMentionSearch", () => {
  it("puts the room session group first and folds in the workspace search's file/commit groups", async () => {
    const search = buildRoomMentionSearch(members, labels, async () => [
      fileGroup([fileItem]),
      agentGroup([agentItem]),
      workspaceSessionGroup(),
      commitGroup(),
    ])
    const groups = await search("")
    expect(groups.map((group) => group.kind)).toEqual([
      "session",
      "file",
      "commit",
    ])
    expect(groups[0].items.map((item) => item.reference.label)).toEqual([
      "@all",
      "@human",
      "Session C",
      "Session D",
    ])
    expect(groups[1].items).toEqual([fileItem])
    expect(groups.some((group) => group.kind === "agent")).toBe(false)
  })

  it("degrades to an unresolvable-folder search without losing the session group", async () => {
    // Mirrors `useReferenceSearch` with `defaultPath: null` — file/commit come
    // back present but empty, never falling back to some other folder's data.
    const search = buildRoomMentionSearch(members, labels, async () => [
      fileGroup([]),
      agentGroup([]),
      workspaceSessionGroup(),
      commitGroup([]),
    ])
    const groups = await search("")
    expect(groups[0].kind).toBe("session")
    expect(groups[0].items).toHaveLength(4) // @all, @human, 2 members
    const file = groups.find((group) => group.kind === "file")
    expect(file?.items).toEqual([])
  })

  it("degrades to just the session group when the workspace search rejects", async () => {
    const search = buildRoomMentionSearch(members, labels, async () => {
      throw new Error("network down")
    })
    const groups = await search("")
    expect(groups).toEqual([buildRoomSessionGroup("", members, labels)])
  })

  it("forwards the query and abort signal to the underlying workspace search", async () => {
    const seen: { query: string; signal: AbortSignal | undefined }[] = []
    const search = buildRoomMentionSearch(
      members,
      labels,
      async (query, signal) => {
        seen.push({ query, signal })
        return []
      }
    )
    const controller = new AbortController()
    await search("readme", controller.signal)
    expect(seen).toEqual([{ query: "readme", signal: controller.signal }])
  })
})
