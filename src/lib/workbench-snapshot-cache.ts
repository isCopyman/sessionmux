import type { OpenedTabsSnapshot } from "@/lib/types"
import type { CacheSnapshot } from "virtua"

/**
 * Ephemeral browser-tab-like view state for one Session surface. The virtua
 * cache keeps measured row heights while `scrollOffset` restores the exact
 * reading position. It deliberately lives only in the bounded warm cache —
 * durable Workbench layout remains backend-owned, while stale scroll geometry
 * disappears with the cached surface that produced it.
 */
export interface WorkbenchSessionViewState {
  scrollOffset: number
  atBottom: boolean
  virtualItemCount: number
  virtualizerCache: CacheSnapshot | null
}

export interface WorkbenchSnapshotCacheEntry {
  workbenchId: number
  snapshot: OpenedTabsSnapshot
  /** ACP connection keys owned by the parked workbench's tab surfaces. */
  connectionContextKeys: readonly string[]
  /**
   * A freshly-created conversation can keep a virtual runtime id after it is
   * bound to its durable conversation row. Remember that association so a
   * warm workbench restore reuses the parsed/live runtime instead of opening a
   * second detail cache under the durable id.
   */
  runtimeConversationIdByTab: Readonly<Record<string, number>>
  /** Per-tab transcript position/measurement state for warm remounts. */
  sessionViewStateByTab?: Readonly<Record<string, WorkbenchSessionViewState>>
}

function cloneViewState(
  state: WorkbenchSessionViewState
): WorkbenchSessionViewState {
  return {
    scrollOffset: state.scrollOffset,
    atBottom: state.atBottom,
    virtualItemCount: state.virtualItemCount,
    // CacheSnapshot is an opaque immutable snapshot from virtua. Retaining the
    // reference is intentional; consumers must never mutate it.
    virtualizerCache: state.virtualizerCache,
  }
}

function cloneEntry(
  entry: WorkbenchSnapshotCacheEntry
): WorkbenchSnapshotCacheEntry {
  return {
    workbenchId: entry.workbenchId,
    snapshot: {
      version: entry.snapshot.version,
      items: entry.snapshot.items.map((item) => ({ ...item })),
    },
    connectionContextKeys: [...entry.connectionContextKeys],
    runtimeConversationIdByTab: {
      ...entry.runtimeConversationIdByTab,
    },
    sessionViewStateByTab: Object.fromEntries(
      Object.entries(entry.sessionViewStateByTab ?? {}).map(
        ([tabId, state]) => [tabId, cloneViewState(state)]
      )
    ),
  }
}

/**
 * Small in-memory LRU for named workbench surfaces. Durable truth still lives
 * in the backend; this cache only lets a recently-used surface paint before
 * its background validation round-trip completes.
 */
export class RecentWorkbenchSnapshotCache {
  private readonly entries = new Map<number, WorkbenchSnapshotCacheEntry>()

  constructor(private capacity = 3) {
    this.assertCapacity(capacity)
  }

  private assertCapacity(capacity: number): void {
    if (!Number.isInteger(capacity) || capacity < 1) {
      throw new Error("Workbench snapshot cache capacity must be positive")
    }
  }

  private trim(): WorkbenchSnapshotCacheEntry[] {
    const evicted: WorkbenchSnapshotCacheEntry[] = []
    while (this.entries.size > this.capacity) {
      const oldestId = this.entries.keys().next().value as number | undefined
      if (oldestId == null) break
      const oldest = this.entries.get(oldestId)
      this.entries.delete(oldestId)
      if (oldest) evicted.push(cloneEntry(oldest))
    }
    return evicted
  }

  /** Resize the browser-like warm set without disturbing the retained order. */
  setCapacity(capacity: number): WorkbenchSnapshotCacheEntry[] {
    this.assertCapacity(capacity)
    this.capacity = capacity
    return this.trim()
  }

  get(workbenchId: number): WorkbenchSnapshotCacheEntry | null {
    const entry = this.entries.get(workbenchId)
    if (!entry) return null
    this.entries.delete(workbenchId)
    this.entries.set(workbenchId, entry)
    return cloneEntry(entry)
  }

  peek(workbenchId: number): WorkbenchSnapshotCacheEntry | null {
    const entry = this.entries.get(workbenchId)
    return entry ? cloneEntry(entry) : null
  }

  getSessionViewState(
    workbenchId: number,
    tabId: string
  ): WorkbenchSessionViewState | null {
    const state = this.entries.get(workbenchId)?.sessionViewStateByTab?.[tabId]
    return state ? cloneViewState(state) : null
  }

  /** Update a mounted tab's view state without changing Workbench LRU order. */
  setSessionViewState(
    workbenchId: number,
    tabId: string,
    state: WorkbenchSessionViewState
  ): boolean {
    const entry = this.entries.get(workbenchId)
    if (!entry || !entry.connectionContextKeys.includes(tabId)) return false
    entry.sessionViewStateByTab = {
      ...(entry.sessionViewStateByTab ?? {}),
      [tabId]: cloneViewState(state),
    }
    return true
  }

  set(entry: WorkbenchSnapshotCacheEntry): WorkbenchSnapshotCacheEntry[] {
    const previous = this.entries.get(entry.workbenchId)
    // The backend version is a monotonic workspace-wide clock. A late request
    // must never walk a workbench cache entry back to an older snapshot.
    if (previous && previous.snapshot.version > entry.snapshot.version) {
      return []
    }

    this.entries.delete(entry.workbenchId)
    this.entries.set(entry.workbenchId, cloneEntry(entry))

    return this.trim()
  }

  delete(workbenchId: number): WorkbenchSnapshotCacheEntry | null {
    const entry = this.entries.get(workbenchId)
    if (!entry) return null
    this.entries.delete(workbenchId)
    return cloneEntry(entry)
  }

  clear(): WorkbenchSnapshotCacheEntry[] {
    const entries = [...this.entries.values()].map(cloneEntry)
    this.entries.clear()
    return entries
  }

  retainedRuntimeConversationIds(): Set<number> {
    const ids = new Set<number>()
    for (const entry of this.entries.values()) {
      for (const item of entry.snapshot.items) {
        if (item.conversation_id == null) continue
        const tabKey = `${item.folder_id}:${item.agent_type}:${item.conversation_id}`
        ids.add(
          entry.runtimeConversationIdByTab[tabKey] ?? item.conversation_id
        )
      }
    }
    return ids
  }

  retainedConnectionContextKeys(): Set<string> {
    const keys = new Set<string>()
    for (const entry of this.entries.values()) {
      for (const key of entry.connectionContextKeys) keys.add(key)
    }
    return keys
  }
}
