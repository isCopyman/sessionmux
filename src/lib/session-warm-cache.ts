export interface WarmSessionEntry {
  runtimeConversationId: number
  connectionContextKeys: readonly string[]
}

function cloneEntry(entry: WarmSessionEntry): WarmSessionEntry {
  return {
    runtimeConversationId: entry.runtimeConversationId,
    connectionContextKeys: [...entry.connectionContextKeys],
  }
}

/**
 * Bounded LRU for heavyweight Session surfaces. Workbench metadata is cheap
 * and kept separately; this cache alone decides which parsed transcript,
 * virtualizer geometry and idle ACP connection remain warm.
 */
export class RecentSessionWarmCache {
  private readonly entries = new Map<number, WarmSessionEntry>()

  constructor(private capacity = 8) {
    this.assertCapacity(capacity)
  }

  private assertCapacity(capacity: number): void {
    if (!Number.isInteger(capacity) || capacity < 0) {
      throw new Error("Session warm cache capacity must be non-negative")
    }
  }

  private trim(
    protectedRuntimeConversationIds: ReadonlySet<number> = new Set()
  ): WarmSessionEntry[] {
    const evicted: WarmSessionEntry[] = []
    while (this.entries.size > this.capacity) {
      const oldestId = [...this.entries.keys()].find(
        (runtimeId) => !protectedRuntimeConversationIds.has(runtimeId)
      )
      if (oldestId == null) break
      const oldest = this.entries.get(oldestId)
      this.entries.delete(oldestId)
      if (oldest) evicted.push(cloneEntry(oldest))
    }
    return evicted
  }

  setCapacity(
    capacity: number,
    protectedRuntimeConversationIds?: ReadonlySet<number>
  ): WarmSessionEntry[] {
    this.assertCapacity(capacity)
    this.capacity = capacity
    return this.trim(protectedRuntimeConversationIds)
  }

  getCapacity(): number {
    return this.capacity
  }

  touch(
    entry: WarmSessionEntry,
    protectedRuntimeConversationIds?: ReadonlySet<number>
  ): WarmSessionEntry[] {
    const existing = this.entries.get(entry.runtimeConversationId)
    const connectionContextKeys = [
      ...new Set([
        ...(existing?.connectionContextKeys ?? []),
        ...entry.connectionContextKeys,
      ]),
    ]
    this.entries.delete(entry.runtimeConversationId)
    this.entries.set(entry.runtimeConversationId, {
      runtimeConversationId: entry.runtimeConversationId,
      connectionContextKeys,
    })
    return this.trim(protectedRuntimeConversationIds)
  }

  hasRuntimeConversationId(runtimeConversationId: number): boolean {
    return this.entries.has(runtimeConversationId)
  }

  hasConnectionContextKey(connectionContextKey: string): boolean {
    for (const entry of this.entries.values()) {
      if (entry.connectionContextKeys.includes(connectionContextKey))
        return true
    }
    return false
  }

  retainedRuntimeConversationIds(): Set<number> {
    return new Set(this.entries.keys())
  }

  retainedConnectionContextKeys(): Set<string> {
    const keys = new Set<string>()
    for (const entry of this.entries.values()) {
      for (const key of entry.connectionContextKeys) keys.add(key)
    }
    return keys
  }

  clear(): WarmSessionEntry[] {
    const entries = [...this.entries.values()].map(cloneEntry)
    this.entries.clear()
    return entries
  }
}
