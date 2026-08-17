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
  outbound: [],
}

/** Persisted Session id for timeline fetch. Draft tabs keep a negative
 * runtime key; Host collaboration rows use the bound DB id. */
export function resolveCollaborationTimelineId(
  conversationId: number | null,
  dbConversationId?: number | null
): number | null {
  if (dbConversationId != null && dbConversationId > 0) return dbConversationId
  return conversationId
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
    // Draft tabs use a negative virtual id until the first prompt persists a
    // real conversation. The Host has no row for those ids.
    if (id == null || id <= 0) return
    const generation = generationRef.current
    try {
      const next = await getCollaborationTimelineProjection(id)
      if (generation === generationRef.current) applyProjection(next)
    } catch (error) {
      if (generation === generationRef.current) {
        const detail =
          error instanceof Error
            ? error.message
            : typeof error === "string"
              ? error
              : JSON.stringify(error)
        console.error("[collaboration] timeline projection:", detail)
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
