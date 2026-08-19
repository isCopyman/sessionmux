"use client"

import { useState, useEffect, useRef, useCallback } from "react"
import { listWorkspaceFiles } from "@/lib/api"
import type { WorkspaceFileEntry } from "@/lib/types"

export interface FlatFileEntry {
  name: string
  /** Relative path from folder root (same as WorkspaceFileEntry.path) */
  relativePath: string
  kind: "file" | "dir"
  /** Pre-computed lowercase relativePath for filtering */
  lowerPath: string
  /** Pre-computed lowercase name for filtering */
  lowerName: string
  /** The root this entry was resolved under (same as WorkspaceFileEntry.root)
   * — `folderPath` for the plain single-folder case, or one of `folderPath`/
   * `additionalPaths` when multiple roots are in play. Needed to join back to
   * an absolute path once a search can span more than one root. */
  root: string
}

interface UseFileTreeOptions {
  folderPath: string | undefined
  /** Extra roots merged into the same search, alongside `folderPath` — a
   * Room's manually-added paths. Omit/leave empty for the plain single-folder
   * case (the file panel, a Session's own composer); their behavior is
   * unchanged by this option existing. */
  additionalPaths?: string[]
  enabled: boolean
}

interface UseFileTreeResult {
  allFiles: FlatFileEntry[]
  loading: boolean
  loaded: boolean
  /** Mark this root combination stale so the next load revalidates it. */
  reset: () => void
}

const EMPTY_ADDITIONAL_PATHS: string[] = []

/** How long a listing is served without revalidating. A workspace walk is not
 * free, and the cost of being at most this far behind is a file created
 * outside the app taking a minute to appear in the `@` panel — cheap next to a
 * filesystem watcher, which is deliberately not on the table. */
const CACHE_TTL_MS = 60_000

/** Every non-empty root, `folderPath` first, then `additionalPaths` in the
 * order given. `null` when there is nothing to search at all. */
function combinedRoots(
  folderPath: string | undefined,
  additionalPaths: readonly string[]
): string[] | null {
  const roots = [folderPath, ...additionalPaths].filter(
    (root): root is string => Boolean(root && root.length > 0)
  )
  return roots.length > 0 ? roots : null
}

/** Stable cache key for a root combination — order-sensitive (there's no need
 * to sort; a different order is treated as a different combination). Joined
 * with the pipe character: reserved in a Windows path and vanishingly
 * unlikely in a POSIX one, so two different root lists essentially cannot
 * collide into the same key the way joining with a space or comma could. */
function rootsKey(roots: string[] | null): string | null {
  return roots ? roots.join("|") : null
}

interface FileTreeCacheEntry {
  files: FlatFileEntry[]
  /** True once a load has succeeded for this key. Distinguishes a genuinely
   * empty workspace from the empty placeholder a first load starts with —
   * `files.length` cannot. */
  hasFiles: boolean
  /** `Date.now()` when `files` landed, or 0 once invalidated. */
  loadedAt: number
  /** The load in flight for this key, shared by every subscriber that asks
   * while it runs. */
  promise: Promise<FlatFileEntry[]> | null
}

/**
 * The listings, keyed by root combination and **shared across every hook
 * instance**. The same folder open in four composer tabs used to mean four
 * full workspace walks of the same tree, each thrown away on unmount; here it
 * is one walk that outlives all of them.
 */
const fileTreeCache = new Map<string, FileTreeCacheEntry>()

/** Mounted hooks, notified by key when a load lands so a listing fetched for
 * one instance immediately reaches the others. */
const cacheListeners = new Set<(key: string) => void>()

function notifyCacheListeners(key: string): void {
  for (const listener of cacheListeners) listener(key)
}

function isStale(entry: FileTreeCacheEntry): boolean {
  return (
    !entry.hasFiles ||
    // Spelled out rather than left to the arithmetic: a test that pins the
    // system clock near the epoch would otherwise read an invalidated entry
    // as fresh.
    entry.loadedAt === 0 ||
    Date.now() - entry.loadedAt >= CACHE_TTL_MS
  )
}

function toFlatEntries(files: WorkspaceFileEntry[]): FlatFileEntry[] {
  return files.map((f) => ({
    name: f.name,
    relativePath: f.path,
    kind: f.kind,
    lowerPath: f.path.toLowerCase(),
    lowerName: f.name.toLowerCase(),
    root: f.root,
  }))
}

/** Load `key`'s listing, or join the load already running for it. The dedup is
 * the point: four composers mounting at once on the same folder must issue one
 * backend walk, not four. */
function loadFileTree(key: string, roots: string[]): Promise<FlatFileEntry[]> {
  const cached = fileTreeCache.get(key)
  if (cached?.promise) return cached.promise

  const [primary, ...extra] = roots
  const promise = listWorkspaceFiles(
    primary,
    extra.length > 0 ? extra : undefined
  )
    .then((files) => {
      const flat = toFlatEntries(files)
      fileTreeCache.set(key, {
        files: flat,
        hasFiles: true,
        loadedAt: Date.now(),
        promise: null,
      })
      notifyCacheListeners(key)
      return flat
    })
    .catch((error) => {
      const entry = fileTreeCache.get(key)
      if (entry?.promise === promise) {
        if (entry.hasFiles) {
          // A failed *revalidation* keeps the listing it failed to refresh —
          // still the best answer available, and still stale, so the next
          // attempt retries.
          fileTreeCache.set(key, { ...entry, promise: null })
        } else {
          // A failed first load has nothing worth remembering; dropping the
          // entry keeps it from reading as "this workspace is empty".
          fileTreeCache.delete(key)
        }
      }
      notifyCacheListeners(key)
      throw error
    })

  fileTreeCache.set(key, {
    files: cached?.files ?? [],
    hasFiles: cached?.hasFiles ?? false,
    loadedAt: cached?.loadedAt ?? 0,
    promise,
  })
  return promise
}

/** Force `key` to revalidate on its next load, keeping the listing usable in
 * the meantime (other subscribers are still rendering it). */
function invalidateFileTree(key: string): void {
  const entry = fileTreeCache.get(key)
  if (entry) fileTreeCache.set(key, { ...entry, loadedAt: 0 })
}

function invalidateAllFileTrees(): void {
  for (const key of fileTreeCache.keys()) invalidateFileTree(key)
}

/**
 * Test-only: drop the shared cache.
 *
 * The cache deliberately outlives every hook instance, which means it also
 * outlives a vitest case: without this, the first test to load a root would
 * hand its listing — and its `listWorkspaceFiles` call count — to every later
 * case in the file. Call it from `beforeEach`.
 */
export function __resetFileTreeCacheForTest(): void {
  fileTreeCache.clear()
  cacheListeners.clear()
}

/**
 * Loads a flat, gitignore-aware listing of every file/dir under `folderPath`
 * plus any `additionalPaths` (lazily, when `enabled`) for in-memory file
 * search — shared by the search dialog and the composer `@`-mention picker.
 *
 * Discovery, gitignore filtering, dedup across roots, and the entry-count /
 * wall-clock budgets all run on the backend in a single `list_workspace_files`
 * call (which prunes ignored directories *during* the walk and applies no
 * depth cap, so deeply nested files are reachable while `node_modules`/
 * `target`/… are never descended).
 *
 * The result is cached per root combination in a **module-level** map, so the
 * hook is really a subscriber: every composer, Room and search dialog pointed
 * at the same roots shares one walk, and the listing survives unmounting.
 * Entries expire after {@link CACHE_TTL_MS} and on window focus, but an
 * expired listing is still rendered immediately while its refresh runs in the
 * background — the panel never waits on the TTL. Switching folders (or a
 * room's additional-path list changing) keeps showing the previous list until
 * the new one loads (`loaded` gates that transition).
 */
export function useFileTree({
  folderPath,
  additionalPaths = EMPTY_ADDITIONAL_PATHS,
  enabled,
}: UseFileTreeOptions): UseFileTreeResult {
  // The published listing, tagged with the roots it belongs to. `loaded` is
  // "this list answers the current roots", so a key switch can keep the
  // previous list on screen (tagged with the old key) until the new one lands.
  const [published, setPublished] = useState<{
    key: string | null
    files: FlatFileEntry[]
  }>({ key: null, files: [] })
  const [loading, setLoading] = useState(false)

  const roots = combinedRoots(folderPath, additionalPaths)
  const key = rootsKey(roots)
  // The focus listener is wired once but must always act on the current roots.
  const syncRef = useRef<() => void>(() => {})

  useEffect(() => {
    if (!enabled || !roots || !key) return
    // Aliased after the guard so the closures below see plain `string` /
    // `string[]` rather than relying on narrowing surviving into them.
    const activeKey = key
    const activeRoots = roots
    let canceled = false

    const publishFromCache = () => {
      const entry = fileTreeCache.get(activeKey)
      if (entry?.hasFiles && !canceled) {
        setPublished({ key: activeKey, files: entry.files })
      }
    }

    const sync = () => {
      if (canceled) return
      // Stale-while-revalidate: an expired listing renders right away and is
      // replaced when the refresh lands.
      publishFromCache()
      const entry = fileTreeCache.get(activeKey)
      if (entry && !isStale(entry)) return
      setLoading(true)
      loadFileTree(activeKey, activeRoots)
        .catch(() => {
          if (canceled) return
          // A failed refresh keeps whatever this instance already shows; a
          // failed first load for these roots has nothing to fall back to, so
          // the list goes empty and unloaded.
          setPublished((prev) =>
            prev.key === activeKey ? prev : { key: null, files: [] }
          )
        })
        .finally(() => {
          if (!canceled) setLoading(false)
        })
    }

    const onCacheChange = (changed: string) => {
      if (changed === activeKey) publishFromCache()
    }
    cacheListeners.add(onCacheChange)
    syncRef.current = sync
    sync()

    return () => {
      canceled = true
      cacheListeners.delete(onCacheChange)
      syncRef.current = () => {}
    }
    // `roots`/`key` are derived fresh every render from
    // `folderPath`/`additionalPaths`; `key` alone is the right dependency
    // (it's what gates re-fetching, pinned by the identity-change test).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, key])

  // Window focus expires every cached listing and revalidates this instance's
  // roots in the background — the same focus-refresh idiom the other data
  // hooks use, and how a file created outside the app shows up without a
  // filesystem watcher. Concurrent instances collapse into one walk per key
  // via the in-flight dedup.
  useEffect(() => {
    const onFocus = () => {
      invalidateAllFileTrees()
      syncRef.current()
    }
    window.addEventListener("focus", onFocus)
    return () => window.removeEventListener("focus", onFocus)
  }, [])

  const reset = useCallback(() => {
    // Invalidate rather than delete: the cache is shared now, and other
    // subscribers rendering this listing should keep it until the refresh
    // lands. As before, this only guarantees the *next* load is a miss — it
    // does not itself re-run the effect.
    if (key) invalidateFileTree(key)
    setPublished({ key: null, files: [] })
  }, [key])

  return {
    allFiles: published.files,
    loading,
    loaded: published.key === key,
    reset,
  }
}
