"use client"

import { useCallback, useRef, useSyncExternalStore } from "react"
import { useConnectionStore } from "@/contexts/acp-connections-context"
import { connectionKeysForSession } from "@/components/workbench/workbench-activity"
import type { ConnectionStatus, WorkTask } from "@/lib/types"

export type TaskActivityDot = "pulse" | "static"

export interface TaskConnectionLookup {
  conversationId: number
  keys: readonly string[]
}

export interface TaskConnectionSnapshot {
  status: ConnectionStatus
}

/**
 * ACP keys for a task's live session, same derivation as the workbench tree
 * (O34): the live connection id if the task still has one, then the
 * canonical `conv-{folder}-{agent}-{conversation}` key. No conversation →
 * no keys; the card then draws no activity dot.
 */
export function connectionKeysForTask(task: WorkTask): string[] {
  if (task.conversation_id == null) return []
  const agentType = task.agent_type ?? task.config?.agent_type ?? null
  if (agentType) {
    return connectionKeysForSession({
      conversationId: task.conversation_id,
      liveTabId: task.connection_id ?? undefined,
      folderId: task.folder_id,
      agentType,
    })
  }
  return task.connection_id ? [task.connection_id] : []
}

/**
 * running + Prompting → green pulse; running but not Prompting → a still
 * dot (idle between turns, or waiting on a tool / child process). Anything
 * else, including a missing `conversation_id`, draws nothing. Pure: the page
 * supplies the connection snapshot.
 */
export function taskActivityDot(
  task: Pick<WorkTask, "status" | "conversation_id">,
  connectionStatus: ConnectionStatus | null | undefined
): TaskActivityDot | null {
  if (task.status !== "running") return null
  if (task.conversation_id == null) return null
  return connectionStatus === "prompting" ? "pulse" : "static"
}

function encodeConnectionSnapshot(
  map: ReadonlyMap<number, TaskConnectionSnapshot>
): string {
  return [...map.entries()]
    .sort((left, right) => left[0] - right[0])
    .map(([id, conn]) => `${id}:${conn.status}`)
    .join(",")
}

/**
 * Subscribe to the ACP connection snapshot for a set of conversations.
 * Copied from the workbench-tree O34 hook so the task board reads the
 * same store without a new backend field.
 */
export function useTaskConnectionsByConversationId(
  lookups: readonly TaskConnectionLookup[]
): ReadonlyMap<number, TaskConnectionSnapshot> {
  const store = useConnectionStore()
  const cacheRef = useRef<{
    encoded: string
    map: Map<number, TaskConnectionSnapshot>
  }>({ encoded: "", map: new Map() })

  const subscribe = useCallback(
    (cb: () => void) => {
      const unsubs = lookups.flatMap((item) =>
        item.keys.map((key) => store.subscribeKey(key, cb))
      )
      return () => {
        for (const unsub of unsubs) unsub()
      }
    },
    [store, lookups]
  )

  const getSnapshot = useCallback(() => {
    const next = new Map<number, TaskConnectionSnapshot>()
    for (const item of lookups) {
      for (const key of item.keys) {
        const conn = store.getConnection(key)
        if (!conn) continue
        next.set(item.conversationId, { status: conn.status })
        break
      }
    }
    const encoded = encodeConnectionSnapshot(next)
    if (encoded === cacheRef.current.encoded) return cacheRef.current.map
    cacheRef.current = { encoded, map: next }
    return next
  }, [store, lookups])

  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
}
