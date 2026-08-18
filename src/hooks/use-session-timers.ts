"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import {
  SESSION_TIMER_CHANGED_EVENT,
  createSessionTimer,
  deleteSessionTimer,
  listSessionTimers,
  resetSessionTimerDelay,
  updateSessionTimer,
} from "@/lib/api"
import { onTransportReconnect, subscribe } from "@/lib/platform"
import type {
  CreateSessionTimerInput,
  SessionTimer,
  UpdateSessionTimerInput,
} from "@/lib/types"
import { randomUUID } from "@/lib/utils"

export interface UseSessionTimersReturn {
  timers: SessionTimer[]
  hydrated: boolean
  create: (input: Omit<CreateSessionTimerInput, "conversationId">) => void
  update: (
    id: string,
    input: Omit<UpdateSessionTimerInput, "expectedUpdatedAt">
  ) => void
  resetDelay: (id: string) => void
  remove: (id: string) => void
}

/**
 * Session Timers (RFC P2) for one conversation. The backend engine is the
 * only trigger authority — this hook never schedules anything locally, it
 * renders the persisted rows and mutates them through the CRUD API,
 * converging on the `session-timer://changed` broadcast.
 */
export function useSessionTimers(
  conversationId: number | null | undefined
): UseSessionTimersReturn {
  const [timers, setTimers] = useState<SessionTimer[]>([])
  const [hydrated, setHydrated] = useState(false)
  const generationRef = useRef(0)

  const reload = useCallback(async () => {
    if (conversationId == null) {
      setTimers([])
      setHydrated(true)
      return
    }
    const generation = ++generationRef.current
    try {
      const next = await listSessionTimers(conversationId)
      if (generation === generationRef.current) setTimers(next)
    } catch (error) {
      console.error("[session-timers] list failed:", error)
    } finally {
      if (generation === generationRef.current) setHydrated(true)
    }
  }, [conversationId])

  useEffect(() => {
    // A conversationless composer (folderless draft, tests) never touches
    // the transport: no list call, no event subscription.
    if (conversationId == null) {
      setTimers([])
      setHydrated(true)
      return
    }
    setTimers([])
    setHydrated(false)
    void reload()

    let disposed = false
    let unsubscribe: (() => void) | undefined
    void subscribe<{ conversationIds: number[] }>(
      SESSION_TIMER_CHANGED_EVENT,
      (payload) => {
        if (disposed) return
        // The engine fires the same event it does for CRUD; refetch unless
        // the broadcast provably targets other conversations.
        if (
          conversationId != null &&
          payload?.conversationIds &&
          !payload.conversationIds.includes(conversationId)
        ) {
          return
        }
        void reload()
      }
    ).then((off) => {
      if (disposed) off()
      else unsubscribe = off
    })
    const offReconnect = onTransportReconnect(() => {
      if (!disposed) void reload()
    })
    return () => {
      disposed = true
      unsubscribe?.()
      offReconnect?.()
    }
  }, [conversationId, reload])

  const create = useCallback(
    (input: Omit<CreateSessionTimerInput, "conversationId">) => {
      if (conversationId == null) return
      void createSessionTimer({
        ...input,
        conversationId,
        clientDedupeId: input.clientDedupeId ?? `timer-${randomUUID()}`,
      })
        .then((timer) => setTimers((prev) => upsert(prev, timer)))
        .catch((error) =>
          console.error("[session-timers] create failed:", error)
        )
    },
    [conversationId]
  )

  const update = useCallback(
    (id: string, input: Omit<UpdateSessionTimerInput, "expectedUpdatedAt">) => {
      if (conversationId == null) return
      const expected = timers.find((timer) => timer.id === id)?.updatedAt
      if (!expected) return
      void updateSessionTimer(conversationId, id, {
        ...input,
        expectedUpdatedAt: expected,
      })
        .then((timer) => setTimers((prev) => upsert(prev, timer)))
        .catch((error) =>
          console.error("[session-timers] update failed:", error)
        )
    },
    [conversationId, timers]
  )

  const resetDelay = useCallback(
    (id: string) => {
      if (conversationId == null) return
      void resetSessionTimerDelay(conversationId, id)
        .then((timer) => setTimers((prev) => upsert(prev, timer)))
        .catch((error) =>
          console.error("[session-timers] reset delay failed:", error)
        )
    },
    [conversationId]
  )

  const remove = useCallback(
    (id: string) => {
      if (conversationId == null) return
      setTimers((prev) => prev.filter((timer) => timer.id !== id))
      void deleteSessionTimer(conversationId, id).catch((error) =>
        console.error("[session-timers] delete failed:", error)
      )
    },
    [conversationId]
  )

  return { timers, hydrated, create, update, resetDelay, remove }
}

function upsert(timers: SessionTimer[], next: SessionTimer): SessionTimer[] {
  const index = timers.findIndex((timer) => timer.id === next.id)
  if (index === -1) return [...timers, next]
  const copy = timers.slice()
  copy[index] = next
  return copy
}
