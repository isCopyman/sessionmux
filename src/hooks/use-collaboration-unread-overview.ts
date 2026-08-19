"use client"

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { getCollaborationUnreadOverview } from "@/lib/api"
import { onTransportReconnect, subscribe } from "@/lib/platform"
import type {
  CollaborationChanged,
  CollaborationUnreadOverview,
  CollaborationUnreadSession,
} from "@/lib/types"
import { COLLABORATION_CHANGED_EVENT } from "./use-collaboration-feed"

/**
 * How long an incoming change waits for company before a refetch is issued.
 *
 * `unread_overview` is a whole-table query with several correlated subqueries
 * per row, and a busy Room emits one collaboration event per message — so a
 * burst used to mean N of those back to back. The window is trailing-only (the
 * first event waits it out with the rest) because nothing here is interactive:
 * a badge landing a quarter-second later is invisible, a stampede is not.
 */
const RELOAD_COALESCE_MS = 250

const EMPTY_OVERVIEW: CollaborationUnreadOverview = {
  totalUnreadCount: 0,
  totalNeedsReplyCount: 0,
  totalAwaitingReplyCount: 0,
  totalFailedCount: 0,
  totalRoomUnreadCount: 0,
  totalRoomNeedsReplyCount: 0,
  sessions: [],
}

export interface UseCollaborationUnreadOverviewReturn {
  overview: CollaborationUnreadOverview
  unreadByConversation: ReadonlyMap<number, number>
  statusByConversation: ReadonlyMap<number, CollaborationUnreadSession>
  hydrated: boolean
  error: unknown
  reload: () => Promise<void>
}

/**
 * Global, backend-authoritative unread projection for the Session index.
 *
 * The hook intentionally stores no optimistic read state. Every window listens
 * for the same collaboration mutation event and replaces its projection from
 * SQLite, so a mark-seen action in one window also clears badges in the others.
 */
export function useCollaborationUnreadOverview(): UseCollaborationUnreadOverviewReturn {
  const [overview, setOverview] =
    useState<CollaborationUnreadOverview>(EMPTY_OVERVIEW)
  const [hydrated, setHydrated] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const requestSequenceRef = useRef(0)

  const reload = useCallback(async () => {
    const requestSequence = ++requestSequenceRef.current
    try {
      const next = await getCollaborationUnreadOverview()
      if (requestSequence !== requestSequenceRef.current) return
      setOverview(next)
      setHydrated(true)
      setError(null)
    } catch (nextError) {
      if (requestSequence !== requestSequenceRef.current) return
      console.error("[collaboration] unread overview:", nextError)
      setHydrated(true)
      setError(nextError)
    }
  }, [])

  const coalesceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // The single entry point for every *incoming* refresh trigger (change events
  // and transport reconnects alike), so they coalesce with each other and not
  // just within their own kind. The caller-facing `reload` stays immediate —
  // an explicit refresh is not a stampede.
  const scheduleReload = useCallback(() => {
    if (coalesceTimerRef.current !== null) {
      clearTimeout(coalesceTimerRef.current)
    }
    coalesceTimerRef.current = setTimeout(() => {
      coalesceTimerRef.current = null
      void reload()
    }, RELOAD_COALESCE_MS)
  }, [reload])

  useEffect(() => {
    let disposed = false
    let unsubscribe: (() => void) | undefined

    // Subscribe before the first snapshot. A message created while the sidebar
    // is mounting is then either present in that snapshot or triggers a later
    // reload; there is no fetch-then-subscribe gap in which it can disappear.
    void subscribe<CollaborationChanged>(
      COLLABORATION_CHANGED_EVENT,
      (change) => {
        if (!disposed && change.conversationIds.length > 0) scheduleReload()
      }
    ).then((off) => {
      if (disposed) {
        off()
        return
      }
      unsubscribe = off
      // Hydration is not coalesced: nothing is on screen yet, so there is
      // nothing to protect and every ms of delay is a visibly empty sidebar.
      void reload()
    })
    const offReconnect = onTransportReconnect(() => {
      if (!disposed) scheduleReload()
    })
    return () => {
      disposed = true
      requestSequenceRef.current += 1
      if (coalesceTimerRef.current !== null) {
        clearTimeout(coalesceTimerRef.current)
        coalesceTimerRef.current = null
      }
      unsubscribe?.()
      offReconnect?.()
    }
  }, [reload, scheduleReload])

  const unreadByConversation = useMemo(() => {
    const counts = new Map<number, number>()
    for (const session of overview.sessions) {
      if (session.unreadCount > 0) {
        counts.set(session.conversationId, session.unreadCount)
      }
    }
    return counts
  }, [overview])

  const statusByConversation = useMemo(
    () =>
      new Map(
        overview.sessions.map((session) => [session.conversationId, session])
      ),
    [overview]
  )

  return {
    overview,
    unreadByConversation,
    statusByConversation,
    hydrated,
    error,
    reload,
  }
}
