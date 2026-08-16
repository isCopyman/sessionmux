"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import {
  dismissCollaborationDelivery,
  getCollaborationFeed,
  getPromptQueue,
  markCollaborationSeen,
  resolveCollaborationObligation,
  restoreCollaborationDelivery,
  retryPromptQueueItem,
} from "@/lib/api"
import { onTransportReconnect, subscribe } from "@/lib/platform"
import type { CollaborationChanged, CollaborationFeed } from "@/lib/types"

export const COLLABORATION_CHANGED_EVENT = "collaboration://changed"

const EMPTY_FEED: CollaborationFeed = {
  conversationId: 0,
  revision: 0,
  unreadCount: 0,
  inbound: [],
  outbound: [],
}

export interface UseCollaborationFeedReturn {
  feed: CollaborationFeed
  hydrated: boolean
  error: unknown
  reload: () => Promise<void>
  markSeen: (deliveryIds: string[]) => Promise<void>
  resolve: (deliveryId: string) => Promise<void>
  dismiss: (deliveryId: string) => Promise<void>
  restore: (deliveryId: string) => Promise<void>
  retry: (queueItemId: string) => Promise<void>
}

export function useCollaborationFeed(
  conversationId: number | null
): UseCollaborationFeedReturn {
  const [feed, setFeed] = useState<CollaborationFeed>(EMPTY_FEED)
  const [hydrated, setHydrated] = useState(conversationId == null)
  const [error, setError] = useState<unknown>(null)
  const conversationIdRef = useRef(conversationId)
  const revisionRef = useRef(0)
  const generationRef = useRef(0)

  useEffect(() => {
    conversationIdRef.current = conversationId
  }, [conversationId])

  const applyFeed = useCallback((next: CollaborationFeed) => {
    if (next.conversationId !== conversationIdRef.current) return
    if (next.revision < revisionRef.current) return
    revisionRef.current = next.revision
    setFeed(next)
    setHydrated(true)
    setError(null)
  }, [])

  const reload = useCallback(async () => {
    const id = conversationIdRef.current
    if (id == null) return
    const generation = generationRef.current
    try {
      const next = await getCollaborationFeed(id)
      if (generation === generationRef.current) applyFeed(next)
    } catch (nextError) {
      if (generation !== generationRef.current) return
      console.error("[collaboration] feed:", nextError)
      setError(nextError)
      setHydrated(true)
    }
  }, [applyFeed])

  useEffect(() => {
    generationRef.current += 1
    const generation = generationRef.current
    revisionRef.current = 0
    setFeed(
      conversationId == null ? EMPTY_FEED : { ...EMPTY_FEED, conversationId }
    )
    setError(null)
    setHydrated(conversationId == null)
    if (conversationId == null) return
    void reload()

    let disposed = false
    let unsubscribe: (() => void) | undefined
    void subscribe<CollaborationChanged>(
      COLLABORATION_CHANGED_EVENT,
      (change) => {
        if (
          !disposed &&
          generation === generationRef.current &&
          change.conversationIds.includes(conversationId)
        ) {
          void reload()
        }
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

  const markSeen = useCallback(
    async (deliveryIds: string[]) => {
      const id = conversationIdRef.current
      if (id == null || deliveryIds.length === 0) return
      try {
        applyFeed(await markCollaborationSeen(id, deliveryIds))
      } catch (nextError) {
        console.error("[collaboration] mark seen:", nextError)
        setError(nextError)
        await reload()
      }
    },
    [applyFeed, reload]
  )

  const dismiss = useCallback(
    async (deliveryId: string) => {
      const id = conversationIdRef.current
      if (id == null) return
      try {
        applyFeed(await dismissCollaborationDelivery(id, deliveryId))
      } catch (nextError) {
        console.error("[collaboration] dismiss:", nextError)
        setError(nextError)
        await reload()
      }
    },
    [applyFeed, reload]
  )

  const resolve = useCallback(
    async (deliveryId: string) => {
      const id = conversationIdRef.current
      if (id == null) return
      try {
        applyFeed(await resolveCollaborationObligation(id, deliveryId))
      } catch (nextError) {
        console.error("[collaboration] resolve obligation:", nextError)
        setError(nextError)
        await reload()
      }
    },
    [applyFeed, reload]
  )

  const retry = useCallback(
    async (queueItemId: string) => {
      const id = conversationIdRef.current
      if (id == null) return
      try {
        const queue = await getPromptQueue(id)
        if (!queue.items.some((item) => item.id === queueItemId)) {
          throw new Error(
            "The failed delivery is no longer in the Session queue"
          )
        }
        await retryPromptQueueItem(id, queueItemId, queue.revision)
        await reload()
      } catch (nextError) {
        console.error("[collaboration] retry:", nextError)
        setError(nextError)
        await reload()
      }
    },
    [reload]
  )

  const restore = useCallback(
    async (deliveryId: string) => {
      const id = conversationIdRef.current
      if (id == null) return
      try {
        applyFeed(await restoreCollaborationDelivery(id, deliveryId))
      } catch (nextError) {
        console.error("[collaboration] restore:", nextError)
        setError(nextError)
        await reload()
      }
    },
    [applyFeed, reload]
  )

  return {
    feed,
    hydrated,
    error,
    reload,
    markSeen,
    resolve,
    dismiss,
    restore,
    retry,
  }
}
