import type { OpenedTabsSnapshot } from "@/lib/types"
import type { CacheSnapshot } from "virtua"

/**
 * Ephemeral browser-tab-like view state for one Session surface. The virtua
 * cache keeps measured row heights while `scrollOffset` restores the exact
 * reading position. It is stored beside lightweight Workbench metadata, but is
 * pruned whenever the independent bounded Session cache lets that surface go.
 * Durable Workbench layout remains backend-owned.
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
  /** Stable tab surface keys captured with this Workbench snapshot. */
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
 * Lightweight in-memory index of visited Workbench surfaces. Durable truth
 * still lives in the backend; keeping these small tab-reference snapshots lets
 * every visited Workbench paint before background validation. Heavy Session
 * state is bounded independently by RecentSessionWarmCache.
 */
export class WorkbenchSnapshotStore {
  private readonly entries = new Map<number, WorkbenchSnapshotCacheEntry>()

  get(workbenchId: number): WorkbenchSnapshotCacheEntry | null {
    const entry = this.entries.get(workbenchId)
    return entry ? cloneEntry(entry) : null
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

  /** Update a mounted tab's ephemeral view state. */
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

  set(entry: WorkbenchSnapshotCacheEntry): void {
    const previous = this.entries.get(entry.workbenchId)
    // The backend version is a monotonic workspace-wide clock. A late request
    // must never walk a workbench cache entry back to an older snapshot.
    if (previous && previous.snapshot.version > entry.snapshot.version) {
      return
    }

    this.entries.set(entry.workbenchId, cloneEntry(entry))
  }

  /** Drop heavy virtualizer geometry when the corresponding Session goes cold. */
  deleteSessionViewStates(connectionContextKeys: ReadonlySet<string>): void {
    if (connectionContextKeys.size === 0) return
    for (const entry of this.entries.values()) {
      const current = entry.sessionViewStateByTab
      if (!current) continue
      const next = Object.fromEntries(
        Object.entries(current).filter(
          ([tabId]) => !connectionContextKeys.has(tabId)
        )
      )
      if (Object.keys(next).length !== Object.keys(current).length) {
        entry.sessionViewStateByTab = next
      }
    }
  }

  /**
   * A bound draft may use a negative runtime id while it is warm. Once that
   * runtime goes cold, future Workbench mounts must fall back to the durable
   * conversation id instead of pointing at a runtime store entry that no
   * longer exists.
   */
  deleteRuntimeConversationIds(
    runtimeConversationIds: ReadonlySet<number>
  ): void {
    if (runtimeConversationIds.size === 0) return
    for (const entry of this.entries.values()) {
      const current = entry.runtimeConversationIdByTab
      const next = Object.fromEntries(
        Object.entries(current).filter(
          ([, runtimeId]) => !runtimeConversationIds.has(runtimeId)
        )
      )
      if (Object.keys(next).length !== Object.keys(current).length) {
        entry.runtimeConversationIdByTab = next
      }
    }
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
}
