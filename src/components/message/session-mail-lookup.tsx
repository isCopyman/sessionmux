"use client"

import { createContext, useContext, useMemo, type ReactNode } from "react"

import type { CollaborationDelivery } from "@/lib/types"

const SessionMailLookupContext = createContext<
  ReadonlyMap<string, CollaborationDelivery>
>(new Map())

export function SessionMailLookupProvider({
  inbound,
  outbound,
  children,
}: {
  inbound: CollaborationDelivery[]
  outbound: CollaborationDelivery[]
  children: ReactNode
}) {
  const byEventId = useMemo(() => {
    const map = new Map<string, CollaborationDelivery>()
    for (const delivery of [...inbound, ...outbound]) {
      if (!map.has(delivery.eventId)) map.set(delivery.eventId, delivery)
    }
    return map
  }, [inbound, outbound])
  return (
    <SessionMailLookupContext.Provider value={byEventId}>
      {children}
    </SessionMailLookupContext.Provider>
  )
}

export function useMailDelivery(
  eventId?: string | null
): CollaborationDelivery | null {
  const map = useContext(SessionMailLookupContext)
  if (!eventId) return null
  return map.get(eventId) ?? null
}
