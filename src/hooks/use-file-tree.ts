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
  /** Clear cached data so the next `enabled=true` triggers a fresh load. */
  reset: () => void
}

const EMPTY_ADDITIONAL_PATHS: string[] = []

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

/**
 * Loads a flat, gitignore-aware listing of every file/dir under `folderPath`
 * plus any `additionalPaths` (lazily, when `enabled`) for in-memory file
 * search — shared by the search dialog and the composer `@`-mention picker.
 *
 * Discovery, gitignore filtering, dedup across roots, and the entry-count /
 * wall-clock budgets all run on the backend in a single `list_workspace_files`
 * call (which prunes ignored directories *during* the walk and applies no
 * depth cap, so deeply nested files are reachable while `node_modules`/
 * `target`/… are never descended). The result is cached per root combination;
 * switching folders (or a room's additional-path list changing) keeps showing
 * the previous list until the new one loads (`loaded` gates that transition).
 */
export function useFileTree({
  folderPath,
  additionalPaths = EMPTY_ADDITIONAL_PATHS,
  enabled,
}: UseFileTreeOptions): UseFileTreeResult {
  const [allFiles, setAllFiles] = useState<FlatFileEntry[]>([])
  const [loading, setLoading] = useState(false)
  const loadedForKeyRef = useRef<string | null>(null)

  const roots = combinedRoots(folderPath, additionalPaths)
  const key = rootsKey(roots)

  useEffect(() => {
    if (!enabled || !roots || !key) return
    if (loadedForKeyRef.current === key) return

    let canceled = false
    setLoading(true)

    async function load() {
      try {
        const [primary, ...extra] = roots!
        const files: WorkspaceFileEntry[] = await listWorkspaceFiles(
          primary,
          extra.length > 0 ? extra : undefined
        )
        const flat: FlatFileEntry[] = files.map((f) => ({
          name: f.name,
          relativePath: f.path,
          kind: f.kind,
          lowerPath: f.path.toLowerCase(),
          lowerName: f.name.toLowerCase(),
          root: f.root,
        }))

        if (!canceled) {
          setAllFiles(flat)
          loadedForKeyRef.current = key
        }
      } catch {
        if (!canceled) setAllFiles([])
      } finally {
        if (!canceled) setLoading(false)
      }
    }

    void load()
    return () => {
      canceled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `roots`/`key` are
    // derived fresh every render from `folderPath`/`additionalPaths`; `key`
    // alone is the right dependency (it's what gates re-fetching).
  }, [enabled, key])

  const reset = useCallback(() => {
    loadedForKeyRef.current = null
    setAllFiles([])
  }, [])

  return {
    allFiles,
    loading,
    loaded: loadedForKeyRef.current === key,
    reset,
  }
}
