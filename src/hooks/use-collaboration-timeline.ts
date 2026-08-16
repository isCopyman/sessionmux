"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { getCollaborationTimelineProjection } from "@/lib/api"
import { onTransportReconnect, subscribe } from "@/lib/platform"
import type {
  CollaborationChanged,
  CollaborationTimelineProjection,
} from "@/lib/types"
import { COLLABORATION_CHANGED_EVENT } from "./use-collaboration-feed"

const EMPTY_PROJECTION: CollaborationTimelineProjection = {
  conversationId: 0,
  revision: 0,
  inbound: [],
}

export function useCollaborationTimeline(conversationId: number | null) {
  const [projection, setProjection] =
    useState<CollaborationTimelineProjection>(EMPTY_PROJECTION)
  const conversationIdRef = useRef(conversationId)
  const revisionRef = useRef(0)
  const generationRef = useRef(0)

  useEffect(() => {
    conversationIdRef.current = conversationId
  }, [conversationId])

  const applyProjection = useCallback(
    (next: CollaborationTimelineProjection) => {
      if (next.conversationId !== conversationIdRef.current) return
      if (next.revision < revisionRef.current) return
      revisionRef.current = next.revision
      setProjection(next)
    },
    []
  )

  const reload = useCallback(async () => {
    const id = conversationIdRef.current
    if (id == null) return
    const generation = generationRef.current
    try {
      const next = await getCollaborationTimelineProjection(id)
      if (generation === generationRef.current) applyProjection(next)
    } catch (error) {
      if (generation === generationRef.current) {
        console.error("[collaboration] timeline projection:", error)
      }
    }
  }, [applyProjection])

  useEffect(() => {
    generationRef.current += 1
    const generation = generationRef.current
    revisionRef.current = 0
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

  if (conversationId == null) return EMPTY_PROJECTION
  return projection.conversationId === conversationId
    ? projection
    : { ...EMPTY_PROJECTION, conversationId }
}
