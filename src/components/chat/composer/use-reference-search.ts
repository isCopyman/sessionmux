"use client"

import { useCallback, useEffect, useLayoutEffect, useRef } from "react"

import { useAcpAgents } from "@/hooks/use-acp-agents"
import { useFileTree, type FlatFileEntry } from "@/hooks/use-file-tree"
import { gitLog, listAllConversations } from "@/lib/api"
import type {
  AcpAgentInfo,
  DbConversationSummary,
  GitLogEntry,
} from "@/lib/types"

import {
  agentToSuggestion,
  commitToSuggestion,
  fileToSuggestion,
  sessionMentionTitle,
  sessionToSuggestion,
} from "./suggestion/adapters"
import type {
  ReferenceSearch,
  SuggestionGroup,
  SuggestionItem,
} from "./suggestion/types"
import type { ReferenceKind } from "./types"

// Commit-synchronous on the client (so the guard-critical refs are updated
// during commit, before any later macrotask/microtask can resolve a stale
// in-flight fetch), but a no-op-safe passive effect during the static-export
// prerender where `useLayoutEffect` would warn.
const useIsomorphicLayoutEffect =
  typeof window !== "undefined" ? useLayoutEffect : useEffect

/** Max rows surfaced per group (mirrors the textarea `@` menu's file cap). */
const MAX_PER_GROUP = 50
/** Stable empty array so an omitted `additionalPaths` doesn't change identity
 * every render and retrigger the sync effect below. */
const EMPTY_ADDITIONAL_PATHS: string[] = []
/** How many commits the git-log group pulls (client-filtered down from here). */
const GIT_LOG_LIMIT = 100
const EMPTY_COMMITS: Promise<GitLogEntry[]> = Promise.resolve([])

/** Display headings for each group; injected so the host can localize them. */
export interface ReferenceGroupLabels {
  file: string
  agent: string
  session: string
  commit: string
  skill: string
}

/**
 * English fallbacks, matching the suggestion popup's `emptyLabel`/`loadingLabel`
 * convention (the host passes localized strings at the integration layer).
 */
export const DEFAULT_GROUP_LABELS: ReferenceGroupLabels = {
  file: "Files",
  agent: "Agents",
  session: "Sessions",
  commit: "Commits",
  skill: "Skills",
}

/** Raw, already-loaded data the pure group builder turns into suggestions. */
export interface ReferenceSearchSources {
  files: FlatFileEntry[]
  /** Workspace root the `files` were loaded under; null disables the group
   * unless `additionalRoots` covers it instead — the file group only needs
   * *some* resolved root, not specifically this one (a Room whose bound
   * folder didn't resolve can still search its additional paths). */
  workspaceRoot: string | null
  /** Room-only: extra roots merged into the same file search alongside
   * `workspaceRoot`. Empty for the plain composer. */
  additionalRoots: string[]
  agents: AcpAgentInfo[]
  sessions: DbConversationSummary[]
  commits: GitLogEntry[]
  /** Repo identity for commit URIs; null disables the commit group. */
  repoKey: string | null
}

/** Case-insensitive substring match against an adapted item's searchable text. */
function suggestionMatches(item: SuggestionItem, lowerQuery: string): boolean {
  if (!lowerQuery) return true
  const ref = item.reference
  return (
    ref.label.toLowerCase().includes(lowerQuery) ||
    ref.id.toLowerCase().includes(lowerQuery) ||
    (item.keywords ?? "").toLowerCase().includes(lowerQuery) ||
    (item.detail ?? "").toLowerCase().includes(lowerQuery)
  )
}

/** Files: filter the (potentially large) list on its pre-lowered fields before
 * paying to adapt the survivors. `truncated` is a cheap boolean — set when a
 * match is found past the cap — so we never scan the whole list for a count.
 * Gated on *some* root existing (primary or additional) rather than only
 * `workspaceRoot`, so a Room can still search its additional paths even when
 * its bound folder itself didn't resolve (R8's "no root → no files" still
 * holds for the plain composer, which never has `additionalRoots`). */
function buildFileGroup(
  q: string,
  sources: ReferenceSearchSources,
  label: string
): SuggestionGroup {
  const items: SuggestionItem[] = []
  let truncated = false
  const hasFileRoot =
    Boolean(sources.workspaceRoot) || sources.additionalRoots.length > 0
  if (hasFileRoot) {
    for (const entry of sources.files) {
      if (q && !entry.lowerName.includes(q) && !entry.lowerPath.includes(q)) {
        continue
      }
      if (items.length >= MAX_PER_GROUP) {
        truncated = true
        break
      }
      items.push(fileToSuggestion(entry))
    }
  }
  return { kind: "file", label, items, truncated }
}

/** Only enabled agents are mentionable — a disabled agent (toggled off in
 * settings) can't be referenced, so it never appears in the `@` panel (its
 * tab count and `truncated` flag follow from this filtered set too). */
function buildAgentGroup(
  q: string,
  sources: ReferenceSearchSources,
  label: string
): SuggestionGroup {
  const matches = sources.agents
    .filter((agent) => agent.enabled)
    .map(agentToSuggestion)
    .filter((item) => suggestionMatches(item, q))
  return {
    kind: "agent",
    label,
    items: matches.slice(0, MAX_PER_GROUP),
    truncated: matches.length > MAX_PER_GROUP,
  }
}

/** Sessions need every title up front (duplicate titles get their `#id`
 * appended), so the titles are folded once into an index-aligned array that
 * feeds both the duplicate count and the adapter — the adapter is never left
 * to recompute a title the count pass already produced. Scanning then stops at
 * the first match past the cap, like the file and commit groups, instead of
 * adapting every session in a list the backend does not bound. */
function buildSessionGroup(
  q: string,
  sources: ReferenceSearchSources,
  label: string
): SuggestionGroup {
  const titles = sources.sessions.map(sessionMentionTitle)
  const titleCounts = new Map<string, number>()
  for (const title of titles) {
    titleCounts.set(title, (titleCounts.get(title) ?? 0) + 1)
  }

  const items: SuggestionItem[] = []
  let truncated = false
  for (let index = 0; index < sources.sessions.length; index++) {
    const title = titles[index]
    const item = sessionToSuggestion(sources.sessions[index], {
      disambiguateId: (titleCounts.get(title) ?? 0) > 1,
      title,
    })
    if (!suggestionMatches(item, q)) continue
    if (items.length >= MAX_PER_GROUP) {
      truncated = true
      break
    }
    items.push(item)
  }
  return { kind: "session", label, items, truncated }
}

function buildCommitGroup(
  q: string,
  sources: ReferenceSearchSources,
  label: string
): SuggestionGroup {
  const items: SuggestionItem[] = []
  let truncated = false
  if (sources.repoKey) {
    const repoKey = sources.repoKey
    for (const entry of sources.commits) {
      const item = commitToSuggestion(entry, repoKey)
      if (!suggestionMatches(item, q)) continue
      if (items.length >= MAX_PER_GROUP) {
        truncated = true
        break
      }
      items.push(item)
    }
  }
  return { kind: "commit", label, items, truncated }
}

/** A previously-built result the builder may narrow instead of rebuilding. */
export interface PreviousReferenceResult {
  /** The raw query string `groups` answered. */
  query: string
  groups: SuggestionGroup[]
}

/**
 * Narrow one already-adapted group to a longer query, or null when that isn't
 * sound and the group has to be rebuilt from the sources.
 *
 * Refusing on `truncated` is the correctness boundary, and it is decided **per
 * group**: a truncated group stopped scanning at {@link MAX_PER_GROUP}, so
 * entries that match the longer query can still be sitting past the cap in the
 * source list and filtering `items` would drop them for good. The file group
 * truncating (50k entries, empty query) must not cost the session group its
 * refinement, which is why this is not a whole-result decision.
 *
 * Re-labels from the caller's current `labels` so a locale switch mid-typing
 * doesn't carry the previous group's heading forward.
 */
function refineGroup(
  previous: SuggestionGroup[] | null,
  kind: ReferenceKind,
  q: string,
  label: string
): SuggestionGroup | null {
  const group = previous?.find((entry) => entry.kind === kind)
  if (!group || group.truncated === true) return null
  return {
    kind,
    label,
    items: group.items.filter((item) => suggestionMatches(item, q)),
    // A non-truncated group held *every* match of the shorter query, so the
    // filtered subset is complete by construction.
    truncated: false,
  }
}

/**
 * Pure: filter + adapt the raw sources into the fixed-order grouped suggestions
 * the `@` panel renders (files → agents → sessions → commits). Each group is
 * independently capped at {@link MAX_PER_GROUP}; empty groups are kept
 * (the popup hides them) so the order is always stable. Extracted from the hook
 * so the matching/ordering/dedup logic is testable without React.
 *
 * `previous` opts into prefix refinement: every matcher here is a plain
 * substring test, so a query that extends the one `previous` answered can only
 * ever match a subset of its items. Passing it turns `re` → `rea` into a filter
 * over ~50 adapted rows instead of a rescan of every file, session and commit.
 * The caller owns freshness — it must only pass a result built from the very
 * same source arrays (see `useReferenceSearch`).
 */
export function buildReferenceGroups(
  query: string,
  sources: ReferenceSearchSources,
  labels: ReferenceGroupLabels = DEFAULT_GROUP_LABELS,
  previous?: PreviousReferenceResult
): SuggestionGroup[] {
  const q = query.trim().toLowerCase()
  const previousQuery = previous ? previous.query.trim().toLowerCase() : null
  const reusable =
    previous && previousQuery !== null && q.startsWith(previousQuery)
      ? previous.groups
      : null

  return [
    refineGroup(reusable, "file", q, labels.file) ??
      buildFileGroup(q, sources, labels.file),
    refineGroup(reusable, "agent", q, labels.agent) ??
      buildAgentGroup(q, sources, labels.agent),
    refineGroup(reusable, "session", q, labels.session) ??
      buildSessionGroup(q, sources, labels.session),
    refineGroup(reusable, "commit", q, labels.commit) ??
      buildCommitGroup(q, sources, labels.commit),
  ]
}

/**
 * Are two source sets literally the same data? Prefix refinement replays a
 * filter over the previous *result*, so it is only sound while nothing behind
 * it has been replaced: a file-tree reload, a focus-busted session cache or a
 * folder switch all swap an array for a new one and must force a full rebuild
 * rather than being filtered away behind a stale row set. Reference equality
 * is exactly the right test for the big arrays (they are React state / cached
 * promise results, stable until they genuinely change); `additionalRoots` is
 * compared by value because a host may rebuild that tiny array per render.
 */
function sameSources(
  a: ReferenceSearchSources,
  b: ReferenceSearchSources
): boolean {
  return (
    a.files === b.files &&
    a.agents === b.agents &&
    a.sessions === b.sessions &&
    a.commits === b.commits &&
    a.workspaceRoot === b.workspaceRoot &&
    a.repoKey === b.repoKey &&
    a.additionalRoots.length === b.additionalRoots.length &&
    a.additionalRoots.every((root, index) => root === b.additionalRoots[index])
  )
}

export interface UseReferenceSearchOptions {
  /**
   * Workspace root for the file + commit groups (and the commit `repoKey`).
   * When empty/null those two groups stay empty while agents/sessions still
   * resolve, so a brand-new draft tab degrades gracefully (R8) — unless
   * `additionalPaths` picks up the file group anyway (see below).
   */
  defaultPath?: string | null
  /**
   * Room-only: extra roots merged into the file group alongside `defaultPath`
   * — a Room's manually-added paths. Does not affect the commit group or its
   * `repoKey`, which stay scoped to `defaultPath` alone (a Room's git log is
   * its bound folder's history, not a blend of unrelated repos). Omit for the
   * plain composer.
   */
  additionalPaths?: string[]
  /**
   * Gates loading. When false the search resolves to empty groups and the file
   * tree is never fetched — let the host pre-warm only the active composer.
   */
  enabled?: boolean
  /** Localized group headings; English fallbacks when omitted. */
  labels?: ReferenceGroupLabels
}

/**
 * Compose the live data sources (file tree, ACP agents, conversations, git log)
 * into a single {@link ReferenceSearch} for the composer's `@` panel. (Skills,
 * commands and experts are inserted via the `/` / `$` triggers and the expert
 * menu, not this panel.)
 *
 * Referential stability is the contract: the suggestion popup re-runs its fetch
 * whenever the `search` identity changes (`suggestion-popup.tsx`), so the
 * returned function is an empty-dependency `useCallback` that reads every source
 * from a ref. A background refresh of any source (e.g. the agent list reloading
 * on window focus) updates the refs but leaves `search` identity untouched — the
 * open panel keeps its results and the user's selection (R7).
 *
 * Files and agents are hook-loaded (and pre-warmed via `enabled`). Sessions and
 * the git log are fetched lazily on the first `@`, key-cached in a ref, and
 * awaited by `search` so the first open is populated without an extra keystroke;
 * window focus busts those caches so they stay fresh.
 */
export function useReferenceSearch({
  defaultPath,
  additionalPaths = EMPTY_ADDITIONAL_PATHS,
  enabled = true,
  labels,
}: UseReferenceSearchOptions): ReferenceSearch {
  const path = defaultPath || null

  const { allFiles, loaded } = useFileTree({
    folderPath: path ?? undefined,
    additionalPaths,
    enabled,
  })
  const { agents } = useAcpAgents()

  // Mirror every changing source into a ref so `search` can stay identity-stable
  // (see the doc comment). Initialized from the first render so the refs are
  // sane even before the sync effect below runs.
  const filesRef = useRef<{
    root: string | null
    additionalRoots: string[]
    files: FlatFileEntry[]
  }>({
    root: null,
    additionalRoots: [],
    files: [],
  })
  const agentsRef = useRef(agents)
  const pathRef = useRef(path)
  const enabledRef = useRef(enabled)
  const labelsRef = useRef(labels)

  // `pathRef` and `enabledRef` gate the post-await freshness check in `search`,
  // so they must reflect the *committed* folder/enabled state synchronously at
  // commit — a passive effect can lag behind a stale in-flight fetch that
  // resolves in the post-commit / pre-effect window, leaking the old folder's
  // commits into the new panel. A layout effect (not a render-phase write) keeps
  // them commit-accurate without updating from an uncommitted transition render.
  useIsomorphicLayoutEffect(() => {
    pathRef.current = path
    enabledRef.current = enabled
  }, [path, enabled])

  useEffect(() => {
    // Only expose files once the tree has loaded for the *current* root
    // combination, so the search never joins the current roots onto a
    // previous folder's (or additional-path list's) relative paths during a
    // switch. Gated on *some* root existing (primary or additional), not just
    // `path`, so a Room can still expose files from its additional paths even
    // when its bound folder didn't resolve.
    const hasAnyRoot = Boolean(path) || additionalPaths.length > 0
    filesRef.current =
      loaded && hasAnyRoot
        ? { root: path, additionalRoots: additionalPaths, files: allFiles }
        : { root: null, additionalRoots: [], files: [] }
    agentsRef.current = agents
    labelsRef.current = labels
  }, [allFiles, loaded, path, additionalPaths, agents, labels])

  // Lazily-fetched network sources, key-cached so repeat searches reuse the
  // in-flight/resolved promise while a folder switch refetches.
  const sessionsRef = useRef<{
    key: string
    promise: Promise<DbConversationSummary[]>
  } | null>(null)
  const commitsRef = useRef<{
    key: string
    promise: Promise<GitLogEntry[]>
  } | null>(null)

  // Last answered result, kept for prefix refinement. It carries the exact
  // sources it was built from; `sameSources` is what keeps a refinement from
  // outliving the data underneath it, so nothing else has to remember to clear
  // this ref when a source reloads.
  const lastResultRef = useRef<{
    query: string
    groups: SuggestionGroup[]
    sources: ReferenceSearchSources
  } | null>(null)

  // Bust the lazy caches when the window regains focus so a session created in
  // another window (or new commits) show up on the next `@` — matching the
  // focus-refresh idiom of the other data hooks, without per-keystroke fetches.
  useEffect(() => {
    const onFocus = () => {
      sessionsRef.current = null
      commitsRef.current = null
    }
    window.addEventListener("focus", onFocus)
    return () => window.removeEventListener("focus", onFocus)
  }, [])

  return useCallback<ReferenceSearch>(async (query, signal) => {
    if (!enabledRef.current) return []

    const path = pathRef.current

    // Lazy session fetch. On rejection the cache entry is cleared (not cached as
    // an empty result) so the next `@` retries instead of wedging on `[]`.
    const sessionsKey = "all"
    let sessionsEntry = sessionsRef.current
    if (sessionsEntry?.key !== sessionsKey) {
      const created: NonNullable<typeof sessionsRef.current> = {
        key: sessionsKey,
        promise: listAllConversations().catch(() => {
          if (sessionsRef.current === created) sessionsRef.current = null
          return [] as DbConversationSummary[]
        }),
      }
      sessionsRef.current = created
      sessionsEntry = created
    }

    // Lazy git-log fetch, keyed by path with the same retry-on-rejection policy.
    let commitsPromise = EMPTY_COMMITS
    if (path) {
      let commitsEntry = commitsRef.current
      if (commitsEntry?.key !== path) {
        const created: NonNullable<typeof commitsRef.current> = {
          key: path,
          promise: gitLog(path, GIT_LOG_LIMIT)
            .then((result) => result.entries)
            .catch(() => {
              if (commitsRef.current === created) commitsRef.current = null
              return [] as GitLogEntry[]
            }),
        }
        commitsRef.current = created
        commitsEntry = created
      }
      commitsPromise = commitsEntry.promise
    } else {
      commitsRef.current = null
    }

    const [sessions, commits] = await Promise.all([
      sessionsEntry.promise,
      commitsPromise,
    ])
    // Discard this result if it can no longer be trusted for the live panel: a
    // newer query aborted us, the composer was disabled, or the workspace folder
    // changed while the network fetch was in flight (the popup only aborts on a
    // query change, so a folder switch would otherwise leak the old repo's
    // commits — built against `path` — into the new folder's panel). The next
    // keystroke re-runs the search against the current folder.
    if (signal?.aborted || !enabledRef.current || pathRef.current !== path) {
      return []
    }

    const fileState = filesRef.current
    const sources: ReferenceSearchSources = {
      files: fileState.files,
      workspaceRoot: fileState.root,
      additionalRoots: fileState.additionalRoots,
      agents: agentsRef.current,
      sessions,
      commits,
      repoKey: path,
    }
    const last = lastResultRef.current
    const groups = buildReferenceGroups(
      query,
      sources,
      labelsRef.current ?? DEFAULT_GROUP_LABELS,
      last && sameSources(last.sources, sources)
        ? { query: last.query, groups: last.groups }
        : undefined
    )
    lastResultRef.current = { query, groups, sources }
    return groups
  }, [])
}
