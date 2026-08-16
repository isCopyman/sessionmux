"use client"

import { useEffect } from "react"
import { onTransportReconnect, subscribe } from "@/lib/platform"
import {
  ORGANIZATION_CHANGED_EVENT,
  type OrganizationChanged,
} from "@/lib/types"
import { useCollectionStore } from "@/stores/collection-store"
import { useOrganizationRevisionStore } from "@/stores/organization-revision-store"
import { useWorkbenchStore } from "@/stores/workbench-store"

function refetchOrganization(change?: OrganizationChanged) {
  useOrganizationRevisionStore.getState().invalidate()
  if (!change || change.entity.startsWith("collection")) {
    void useCollectionStore.getState().hydrate(true)
  }
  if (!change || change.entity === "workbench") {
    void useWorkbenchStore.getState().hydrate()
  }
}

/** Reconciles headless Host Control organization writes without navigation.
 * It only refetches backend facts; it never mounts or focuses a Workbench. */
export function OrganizationSyncListener() {
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    void subscribe<OrganizationChanged>(
      ORGANIZATION_CHANGED_EVENT,
      refetchOrganization
    ).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    })
    const offReconnect = onTransportReconnect(() => refetchOrganization())
    return () => {
      disposed = true
      unlisten?.()
      offReconnect?.()
    }
  }, [])
  return null
}
