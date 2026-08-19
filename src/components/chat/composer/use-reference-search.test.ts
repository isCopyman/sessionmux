import { act, renderHook } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import type { FlatFileEntry } from "@/hooks/use-file-tree"
import type {
  AcpAgentInfo,
  DbConversationSummary,
  GitLogEntry,
} from "@/lib/types"

import type { ReferenceKind } from "./types"
import type { SuggestionGroup } from "./suggestion/types"
import {
  buildReferenceGroups,
  DEFAULT_GROUP_LABELS,
  useReferenceSearch,
  type ReferenceSearchSources,
} from "./use-reference-search"

// The adapters are spied (call-through) so the prefix-refinement and
// stop-at-the-cap tests can assert on work *not* done — "did it re-adapt?" is
// the only externally visible difference between a refined and a rebuilt
// group, which otherwise hold identical items.
vi.mock("./suggestion/adapters", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./suggestion/adapters")>()
  return {
    ...actual,
    fileToSuggestion: vi.fn(actual.fileToSuggestion),
    agentToSuggestion: vi.fn(actual.agentToSuggestion),
    sessionToSuggestion: vi.fn(actual.sessionToSuggestion),
    commitToSuggestion: vi.fn(actual.commitToSuggestion),
    sessionMentionTitle: vi.fn(actual.sessionMentionTitle),
  }
})

import {
  fileToSuggestion,
  sessionMentionTitle,
  sessionToSuggestion,
} from "./suggestion/adapters"

beforeEach(() => {
  // `mockClear`, not `mockReset`: these keep the real implementation.
  vi.mocked(fileToSuggestion).mockClear()
  vi.mocked(sessionMentionTitle).mockClear()
  vi.mocked(sessionToSuggestion).mockClear()
})

// --- fixtures ---------------------------------------------------------------

function makeFile(
  relativePath: string,
  kind: "file" | "dir" = "file",
  root = "/repo"
): FlatFileEntry {
  const name = relativePath.split("/").pop() ?? relativePath
  return {
    name,
    relativePath,
    kind,
    lowerPath: relativePath.toLowerCase(),
    lowerName: name.toLowerCase(),
    root,
  }
}

function makeAgent(
  agentType: string,
  over: { name?: string; description?: string; enabled?: boolean } = {}
): AcpAgentInfo {
  return {
    agent_type: agentType,
    name: over.name ?? agentType,
    description: over.description ?? "",
    available: true,
    enabled: over.enabled ?? true,
    sort_order: 0,
  } as unknown as AcpAgentInfo
}

function makeConversation(id: number, title: string): DbConversationSummary {
  return {
    id,
    title,
    agent_type: "claude_code",
    status: "idle",
    git_branch: null,
  } as unknown as DbConversationSummary
}

function makeCommit(
  hash: string,
  message = "msg",
  author = "Dev"
): GitLogEntry {
  return {
    hash,
    full_hash: `${hash}0000`,
    author,
    date: "2026-01-01",
    message,
    files: [],
    pushed: false,
  }
}

function emptySources(
  over: Partial<ReferenceSearchSources> = {}
): ReferenceSearchSources {
  return {
    files: [],
    workspaceRoot: null,
    additionalRoots: [],
    agents: [],
    sessions: [],
    commits: [],
    repoKey: null,
    ...over,
  }
}

const itemsOf = (groups: SuggestionGroup[], kind: ReferenceKind) =>
  groups.find((g) => g.kind === kind)?.items ?? []

// --- pure builder -----------------------------------------------------------

describe("buildReferenceGroups", () => {
  it("returns the four groups in a fixed order (no skill group)", () => {
    const groups = buildReferenceGroups("", emptySources())
    expect(groups.map((g) => g.kind)).toEqual([
      "file",
      "agent",
      "session",
      "commit",
    ])
  })

  it("keeps every group present (empty groups are not dropped)", () => {
    const groups = buildReferenceGroups("", emptySources())
    expect(groups).toHaveLength(4)
    expect(groups.every((g) => g.items.length === 0)).toBe(true)
  })

  it("defaults the group headings to the English labels", () => {
    const groups = buildReferenceGroups("", emptySources())
    expect(groups.map((g) => g.label)).toEqual([
      DEFAULT_GROUP_LABELS.file,
      DEFAULT_GROUP_LABELS.agent,
      DEFAULT_GROUP_LABELS.session,
      DEFAULT_GROUP_LABELS.commit,
    ])
  })

  it("accepts injected (localized) group headings", () => {
    const labels = {
      file: "文件",
      agent: "智能体",
      session: "会话",
      commit: "提交",
      skill: "技能",
    }
    const groups = buildReferenceGroups("", emptySources(), labels)
    expect(itemsOf(groups, "file")).toBeDefined()
    expect(groups.find((g) => g.kind === "agent")?.label).toBe("智能体")
  })

  it("adapts files into file:// references rooted at the workspace", () => {
    const groups = buildReferenceGroups(
      "",
      emptySources({
        files: [makeFile("a.ts"), makeFile("src/app.ts")],
        workspaceRoot: "/repo",
      })
    )
    const files = itemsOf(groups, "file")
    expect(files).toHaveLength(2)
    expect(files.map((f) => f.reference.uri)).toEqual([
      "file:///repo/a.ts",
      "file:///repo/src/app.ts",
    ])
  })

  it("filters files by name or relative path, case-insensitively", () => {
    const groups = buildReferenceGroups(
      "APP",
      emptySources({
        files: [makeFile("a.ts"), makeFile("src/App.tsx")],
        workspaceRoot: "/repo",
      })
    )
    const files = itemsOf(groups, "file")
    expect(files).toHaveLength(1)
    expect(files[0].reference.id).toBe("src/App.tsx")
  })

  it("omits the file group when there is no workspace root (R8)", () => {
    const groups = buildReferenceGroups(
      "",
      emptySources({ files: [makeFile("a.ts")], workspaceRoot: null })
    )
    expect(itemsOf(groups, "file")).toHaveLength(0)
  })

  it("still shows files from additionalRoots when workspaceRoot is null (Room whose bound folder didn't resolve)", () => {
    const groups = buildReferenceGroups(
      "",
      emptySources({
        files: [makeFile("notes.md", "file", "/extra")],
        workspaceRoot: null,
        additionalRoots: ["/extra"],
      })
    )
    const files = itemsOf(groups, "file")
    expect(files).toHaveLength(1)
    expect(files[0].reference.uri).toBe("file:///extra/notes.md")
  })

  it("merges files from multiple roots and keeps each entry's own root in its uri", () => {
    const groups = buildReferenceGroups(
      "",
      emptySources({
        files: [
          makeFile("README.md", "file", "/repo"),
          makeFile("README.md", "file", "/extra"),
        ],
        workspaceRoot: "/repo",
        additionalRoots: ["/extra"],
      })
    )
    const files = itemsOf(groups, "file")
    expect(files).toHaveLength(2)
    expect(files.map((f) => f.reference.uri).sort()).toEqual([
      "file:///extra/README.md",
      "file:///repo/README.md",
    ])
  })

  it("filters agents by name / type / description", () => {
    const groups = buildReferenceGroups(
      "codex",
      emptySources({
        agents: [
          makeAgent("codex", { name: "Codex" }),
          makeAgent("gemini", { name: "Gemini" }),
        ],
      })
    )
    const agents = itemsOf(groups, "agent")
    expect(agents).toHaveLength(1)
    expect(agents[0].reference.id).toBe("codex")
  })

  it("excludes disabled agents (only enabled agents are mentionable)", () => {
    const groups = buildReferenceGroups(
      "",
      emptySources({
        agents: [
          makeAgent("codex", { name: "Codex" }),
          makeAgent("gemini", { name: "Gemini", enabled: false }),
        ],
      })
    )
    const agents = itemsOf(groups, "agent")
    expect(agents).toHaveLength(1)
    expect(agents[0].reference.id).toBe("codex")
  })

  it("adapts sessions into codeg://session references", () => {
    const groups = buildReferenceGroups(
      "login",
      emptySources({
        sessions: [
          makeConversation(7, "Login refactor"),
          makeConversation(8, "Sidebar perf"),
        ],
      })
    )
    const sessions = itemsOf(groups, "session")
    expect(sessions).toHaveLength(1)
    expect(sessions[0].reference.uri).toBe("codeg://session/7")
  })

  it("disambiguates two Sessions that share a title", () => {
    const groups = buildReferenceGroups(
      "",
      emptySources({
        sessions: [
          makeConversation(7, "Reviewer"),
          makeConversation(8, "Reviewer"),
        ],
      })
    )
    const sessions = itemsOf(groups, "session")
    expect(sessions.map((item) => item.reference.label).sort()).toEqual([
      "Reviewer #7",
      "Reviewer #8",
    ])
  })

  it("omits the commit group when there is no repoKey (R8)", () => {
    const groups = buildReferenceGroups(
      "",
      emptySources({ commits: [makeCommit("abc1234")], repoKey: null })
    )
    expect(itemsOf(groups, "commit")).toHaveLength(0)
  })

  it("adapts commits and filters by hash / message / author", () => {
    const groups = buildReferenceGroups(
      "bugfix",
      emptySources({
        commits: [
          makeCommit("abc1234", "bugfix: crash"),
          makeCommit("def5678", "feature"),
        ],
        repoKey: "/repo",
      })
    )
    const commits = itemsOf(groups, "commit")
    expect(commits).toHaveLength(1)
    expect(commits[0].reference.uri).toBe("codeg://commit/%2Frepo@abc12340000")
  })

  it("caps each group at 50 items and flags the overflow as truncated", () => {
    const files = Array.from({ length: 60 }, (_, i) => makeFile(`f${i}.ts`))
    const groups = buildReferenceGroups(
      "",
      emptySources({ files, workspaceRoot: "/repo" })
    )
    const fileGroup = groups.find((g) => g.kind === "file")
    expect(fileGroup?.items).toHaveLength(50)
    expect(fileGroup?.truncated).toBe(true)
  })

  it("does not flag truncation when a group exactly fills the cap", () => {
    const files = Array.from({ length: 50 }, (_, i) => makeFile(`f${i}.ts`))
    const groups = buildReferenceGroups(
      "",
      emptySources({ files, workspaceRoot: "/repo" })
    )
    const fileGroup = groups.find((g) => g.kind === "file")
    expect(fileGroup?.items).toHaveLength(50)
    expect(fileGroup?.truncated).toBe(false)
  })

  it("flags truncation for slice-based groups (agents) as well", () => {
    const agents = Array.from({ length: 51 }, (_, i) => makeAgent(`a${i}`))
    const groups = buildReferenceGroups("", emptySources({ agents }))
    const agentGroup = groups.find((g) => g.kind === "agent")
    expect(agentGroup?.items).toHaveLength(50)
    expect(agentGroup?.truncated).toBe(true)
  })

  it("returns everything for an empty query (whitespace-trimmed)", () => {
    const groups = buildReferenceGroups(
      "   ",
      emptySources({
        agents: [makeAgent("codex"), makeAgent("gemini")],
      })
    )
    expect(itemsOf(groups, "agent")).toHaveLength(2)
  })
})

// --- session group: one title fold, stop at the cap (P0-3) -------------------

describe("buildReferenceGroups session group", () => {
  it("folds each session title exactly once per build", () => {
    const sessions = [
      makeConversation(1, "Reviewer"),
      makeConversation(2, "Reviewer"),
      makeConversation(3, "Planner"),
    ]
    buildReferenceGroups("", emptySources({ sessions }))
    // One pass over the list, feeding both the duplicate count and the
    // adapter — not one pass to count and another to adapt.
    expect(sessionMentionTitle).toHaveBeenCalledTimes(3)
  })

  it("hands the adapter the title it already folded", () => {
    const sessions = [makeConversation(7, "Login refactor")]
    buildReferenceGroups("", emptySources({ sessions }))
    expect(sessionToSuggestion).toHaveBeenCalledWith(sessions[0], {
      disambiguateId: false,
      title: "Login refactor",
    })
  })

  it("stops adapting sessions once the cap is filled", () => {
    // The backend session list has no LIMIT, so an unbounded scan here is a
    // per-keystroke cost proportional to the whole database.
    const sessions = Array.from({ length: 200 }, (_, i) =>
      makeConversation(i + 1, `Session ${i + 1}`)
    )
    const groups = buildReferenceGroups("session", emptySources({ sessions }))
    const group = groups.find((g) => g.kind === "session")
    expect(group?.items).toHaveLength(50)
    expect(group?.truncated).toBe(true)
    // 50 kept plus the 51st, which is what proved the overflow.
    expect(sessionToSuggestion).toHaveBeenCalledTimes(51)
  })

  it("counts duplicate titles across the whole list, including past the cap", () => {
    // The `#id` suffixes on the 50 shown rows are decided by a count that saw
    // the 10 the adapt scan never reached — the count pass stays complete.
    const sessions = Array.from({ length: 60 }, (_, i) =>
      makeConversation(i + 1, "Reviewer")
    )
    const items = itemsOf(
      buildReferenceGroups("", emptySources({ sessions })),
      "session"
    )
    expect(items).toHaveLength(50)
    expect(items[0].reference.label).toBe("Reviewer #1")
  })
})

// --- prefix refinement (P0-2) ------------------------------------------------

describe("buildReferenceGroups prefix refinement", () => {
  it("narrows the previous result instead of rescanning the sources", () => {
    const sources = emptySources({
      sessions: [
        makeConversation(1, "Reviewer"),
        makeConversation(2, "Router"),
      ],
    })
    const first = buildReferenceGroups("r", sources)
    expect(itemsOf(first, "session")).toHaveLength(2)

    vi.mocked(sessionToSuggestion).mockClear()
    const second = buildReferenceGroups("re", sources, DEFAULT_GROUP_LABELS, {
      query: "r",
      groups: first,
    })
    expect(itemsOf(second, "session").map((i) => i.reference.id)).toEqual(["1"])
    expect(sessionToSuggestion).not.toHaveBeenCalled()
  })

  it("rebuilds a truncated group — its matches can sit past the cap", () => {
    // f0…f59 all match "f", so the group stops at f49 and never adapts
    // f50…f59 — which are exactly the rows "f5" needs.
    const files = Array.from({ length: 60 }, (_, i) => makeFile(`f${i}.ts`))
    const sources = emptySources({ files, workspaceRoot: "/repo" })
    const first = buildReferenceGroups("f", sources)
    expect(first.find((g) => g.kind === "file")?.truncated).toBe(true)

    vi.mocked(fileToSuggestion).mockClear()
    const second = buildReferenceGroups("f5", sources, DEFAULT_GROUP_LABELS, {
      query: "f",
      groups: first,
    })
    // f5.ts + f50…f59; refining the previous items would have found only f5.
    expect(itemsOf(second, "file")).toHaveLength(11)
    expect(fileToSuggestion).toHaveBeenCalled()
  })

  it("decides truncation per group, so one capped group does not block the rest", () => {
    const sources = emptySources({
      files: Array.from({ length: 60 }, (_, i) => makeFile(`f${i}.ts`)),
      workspaceRoot: "/repo",
      sessions: [makeConversation(1, "Fixture"), makeConversation(2, "Folder")],
    })
    const first = buildReferenceGroups("f", sources)

    vi.mocked(fileToSuggestion).mockClear()
    vi.mocked(sessionToSuggestion).mockClear()
    buildReferenceGroups("fi", sources, DEFAULT_GROUP_LABELS, {
      query: "f",
      groups: first,
    })
    // Files rebuilt (truncated), sessions refined (not truncated).
    expect(fileToSuggestion).toHaveBeenCalled()
    expect(sessionToSuggestion).not.toHaveBeenCalled()
  })

  it("rebuilds when the new query is not an extension of the previous one", () => {
    const sources = emptySources({
      sessions: [
        makeConversation(1, "Reviewer"),
        makeConversation(2, "Router"),
      ],
    })
    const first = buildReferenceGroups("re", sources)

    vi.mocked(sessionToSuggestion).mockClear()
    const second = buildReferenceGroups("r", sources, DEFAULT_GROUP_LABELS, {
      query: "re",
      groups: first,
    })
    expect(sessionToSuggestion).toHaveBeenCalled()
    expect(itemsOf(second, "session")).toHaveLength(2)
  })

  it("refines case-insensitively, matching the full-build semantics", () => {
    const sources = emptySources({
      files: [makeFile("src/App.tsx"), makeFile("src/api.ts")],
      workspaceRoot: "/repo",
    })
    const first = buildReferenceGroups("A", sources)
    const refined = buildReferenceGroups("APP", sources, DEFAULT_GROUP_LABELS, {
      query: "A",
      groups: first,
    })
    expect(itemsOf(refined, "file").map((i) => i.reference.id)).toEqual(
      itemsOf(buildReferenceGroups("APP", sources), "file").map(
        (i) => i.reference.id
      )
    )
    expect(itemsOf(refined, "file")).toHaveLength(1)
  })

  it("re-labels a refined group so a locale switch still lands", () => {
    const sources = emptySources({
      sessions: [makeConversation(1, "Reviewer")],
    })
    const first = buildReferenceGroups("r", sources)
    const localized = { ...DEFAULT_GROUP_LABELS, session: "会话" }
    const second = buildReferenceGroups("re", sources, localized, {
      query: "r",
      groups: first,
    })
    expect(second.find((g) => g.kind === "session")?.label).toBe("会话")
  })
})

// --- hook --------------------------------------------------------------------

const mocks = vi.hoisted(() => ({
  agents: [] as AcpAgentInfo[],
  files: { allFiles: [] as FlatFileEntry[], loaded: false },
  listAllConversations: vi.fn(),
  gitLog: vi.fn(),
}))

vi.mock("@/hooks/use-file-tree", () => ({
  useFileTree: () => ({
    allFiles: mocks.files.allFiles,
    loaded: mocks.files.loaded,
    loading: false,
    reset: () => {},
  }),
}))
vi.mock("@/hooks/use-acp-agents", () => ({
  useAcpAgents: () => ({ agents: mocks.agents, fresh: true, refresh: vi.fn() }),
}))
vi.mock("@/lib/api", () => ({
  listAllConversations: (...args: unknown[]) =>
    mocks.listAllConversations(...args),
  gitLog: (...args: unknown[]) => mocks.gitLog(...args),
}))

describe("useReferenceSearch", () => {
  beforeEach(() => {
    mocks.agents = []
    mocks.files = { allFiles: [], loaded: false }
    mocks.listAllConversations.mockReset().mockResolvedValue([])
    mocks.gitLog
      .mockReset()
      .mockResolvedValue({ entries: [], has_upstream: false })
  })

  it("returns a referentially stable search across data-source updates (R7)", async () => {
    mocks.agents = [makeAgent("codex", { name: "Codex" })]
    const { result, rerender } = renderHook(
      (props: { enabled: boolean }) => useReferenceSearch(props),
      { initialProps: { enabled: true } }
    )
    const first = result.current

    // A background refresh swaps the agents array reference.
    mocks.agents = [
      makeAgent("codex", { name: "Codex" }),
      makeAgent("gemini", { name: "Gemini" }),
    ]
    rerender({ enabled: true })

    // Identity is unchanged — the popup will not re-fetch or reset selection…
    expect(result.current).toBe(first)
    // …yet the stable function reads the freshest data through its refs.
    let groups!: SuggestionGroup[]
    await act(async () => {
      groups = (await result.current("")) as SuggestionGroup[]
    })
    expect(itemsOf(groups, "agent")).toHaveLength(2)
  })

  it("lazily fetches and awaits sessions + commits on the first search", async () => {
    mocks.listAllConversations.mockResolvedValue([
      makeConversation(7, "Login refactor"),
    ])
    mocks.gitLog.mockResolvedValue({
      entries: [makeCommit("abc1234", "fix")],
      has_upstream: false,
    })
    mocks.files = { allFiles: [makeFile("a.ts")], loaded: true }

    const { result } = renderHook(() =>
      useReferenceSearch({ defaultPath: "/repo", enabled: true })
    )
    let groups!: SuggestionGroup[]
    await act(async () => {
      groups = (await result.current("")) as SuggestionGroup[]
    })

    expect(mocks.listAllConversations).toHaveBeenCalledTimes(1)
    expect(mocks.gitLog).toHaveBeenCalledWith("/repo", 100)
    expect(itemsOf(groups, "session")).toHaveLength(1)
    expect(itemsOf(groups, "commit")).toHaveLength(1)
    expect(itemsOf(groups, "file")).toHaveLength(1)
  })

  it("reuses the cached network promises across repeated searches", async () => {
    const { result } = renderHook(() =>
      useReferenceSearch({ defaultPath: "/repo", enabled: true })
    )
    await act(async () => {
      await result.current("")
      await result.current("a")
      await result.current("ab")
    })
    expect(mocks.listAllConversations).toHaveBeenCalledTimes(1)
    expect(mocks.gitLog).toHaveBeenCalledTimes(1)
  })

  it("resolves to no groups and touches no network when disabled", async () => {
    const { result } = renderHook(() =>
      useReferenceSearch({ defaultPath: "/repo", enabled: false })
    )
    let groups!: SuggestionGroup[]
    await act(async () => {
      groups = (await result.current("x")) as SuggestionGroup[]
    })
    expect(groups).toEqual([])
    expect(mocks.listAllConversations).not.toHaveBeenCalled()
    expect(mocks.gitLog).not.toHaveBeenCalled()
  })

  it("returns no groups when the query is aborted mid-fetch", async () => {
    mocks.listAllConversations.mockImplementation(
      () =>
        new Promise((resolve) =>
          setTimeout(() => resolve([makeConversation(1, "x")]), 10)
        )
    )
    const { result } = renderHook(() =>
      useReferenceSearch({ defaultPath: "/repo", enabled: true })
    )
    let groups!: SuggestionGroup[]
    await act(async () => {
      const controller = new AbortController()
      const pending = result.current("", controller.signal)
      controller.abort()
      groups = (await pending) as SuggestionGroup[]
    })
    expect(groups).toEqual([])
  })

  it("degrades gracefully with no workspace path: agents resolve, files/commits stay empty (R8)", async () => {
    mocks.agents = [makeAgent("codex", { name: "Codex" })]
    mocks.files = { allFiles: [makeFile("a.ts")], loaded: true }

    const { result } = renderHook(() => useReferenceSearch({ enabled: true }))
    let groups!: SuggestionGroup[]
    await act(async () => {
      groups = (await result.current("")) as SuggestionGroup[]
    })

    expect(itemsOf(groups, "file")).toHaveLength(0)
    expect(itemsOf(groups, "commit")).toHaveLength(0)
    expect(itemsOf(groups, "agent")).toHaveLength(1)
    expect(mocks.gitLog).not.toHaveBeenCalled()
  })

  it("does not leak the previous folder's commits when defaultPath changes mid-fetch", async () => {
    // git-log for repo A hangs until we resolve it by hand; repo B resolves
    // immediately. We switch folders before A resolves.
    let resolveA!: (value: {
      entries: GitLogEntry[]
      has_upstream: boolean
    }) => void
    mocks.gitLog.mockImplementation((repoPath: string) => {
      if (repoPath === "/repoA") {
        return new Promise((resolve) => {
          resolveA = resolve
        })
      }
      return Promise.resolve({
        entries: [makeCommit("bbb", "repo B commit")],
        has_upstream: false,
      })
    })

    const { result, rerender } = renderHook(
      (props: { defaultPath: string }) =>
        useReferenceSearch({ defaultPath: props.defaultPath, enabled: true }),
      { initialProps: { defaultPath: "/repoA" } }
    )

    // Start a search that hangs on gitLog("/repoA").
    let pending!: SuggestionGroup[] | Promise<SuggestionGroup[]>
    await act(async () => {
      pending = result.current("")
    })

    // The composer switches to repo B; commit flushes the ref mirror (pathRef →
    // "/repoB") before the stale repo A fetch resolves.
    //
    // NOTE: jsdom + RTL `act()` flush BOTH layout and passive effects at their
    // boundaries, so a unit test cannot reproduce the real-browser window where
    // a passive effect lags behind a macrotask that resolves the stale promise.
    // This asserts the guard CONTRACT (a folder switch discards the stale
    // result); the production fix that closes the timing window is the
    // commit-synchronous layout-effect mirror of pathRef in the hook.
    await act(async () => {
      rerender({ defaultPath: "/repoB" })
    })

    let groups!: SuggestionGroup[]
    await act(async () => {
      resolveA({
        entries: [makeCommit("aaa", "repo A commit")],
        has_upstream: false,
      })
      groups = (await pending) as SuggestionGroup[]
    })

    // The stale invocation must not render repo A commits into repo B's panel;
    // it bails so the next keystroke re-queries the current folder.
    expect(groups).toEqual([])
  })

  it("reuses the previous result for a prefix keystroke", async () => {
    mocks.listAllConversations.mockResolvedValue([
      makeConversation(1, "Reviewer"),
      makeConversation(2, "Router"),
    ])
    const { result } = renderHook(() => useReferenceSearch({ enabled: true }))
    await act(async () => {
      await result.current("r")
    })
    const adaptedForR = vi.mocked(sessionToSuggestion).mock.calls.length
    expect(adaptedForR).toBeGreaterThan(0)

    let groups!: SuggestionGroup[]
    await act(async () => {
      groups = (await result.current("re")) as SuggestionGroup[]
    })
    // Nothing re-adapted: the second keystroke filtered the rows the first one
    // already built.
    expect(vi.mocked(sessionToSuggestion).mock.calls.length).toBe(adaptedForR)
    expect(itemsOf(groups, "session").map((i) => i.reference.id)).toEqual(["1"])
  })

  it("rebuilds a prefix keystroke when a source reloaded underneath it", async () => {
    mocks.listAllConversations.mockResolvedValue([
      makeConversation(1, "Reviewer"),
    ])
    const { result } = renderHook(() => useReferenceSearch({ enabled: true }))
    await act(async () => {
      await result.current("r")
    })
    const adaptedForR = vi.mocked(sessionToSuggestion).mock.calls.length

    // Window focus busts the session cache, so the next search resolves a
    // *different* array. Filtering the old result would hide the new row for
    // as long as the user keeps extending the query.
    mocks.listAllConversations.mockResolvedValue([
      makeConversation(1, "Reviewer"),
      makeConversation(3, "Rebase fix"),
    ])
    act(() => {
      window.dispatchEvent(new Event("focus"))
    })

    let groups!: SuggestionGroup[]
    await act(async () => {
      groups = (await result.current("re")) as SuggestionGroup[]
    })
    expect(vi.mocked(sessionToSuggestion).mock.calls.length).toBeGreaterThan(
      adaptedForR
    )
    expect(itemsOf(groups, "session")).toHaveLength(2)
  })

  it("retries a lazy fetch after it rejects (a failure is never cached)", async () => {
    mocks.listAllConversations
      .mockRejectedValueOnce(new Error("transient"))
      .mockResolvedValueOnce([makeConversation(1, "Recovered")])

    const { result } = renderHook(() => useReferenceSearch({ enabled: true }))

    let first!: SuggestionGroup[]
    await act(async () => {
      first = (await result.current("")) as SuggestionGroup[]
    })
    // First fetch rejected → the session group is empty, but the others render.
    expect(itemsOf(first, "session")).toHaveLength(0)

    let second!: SuggestionGroup[]
    await act(async () => {
      second = (await result.current("")) as SuggestionGroup[]
    })
    // The failure was not cached, so the second `@` issues a fresh request…
    expect(mocks.listAllConversations).toHaveBeenCalledTimes(2)
    // …which succeeds and populates the group.
    expect(itemsOf(second, "session")).toHaveLength(1)
  })
})
