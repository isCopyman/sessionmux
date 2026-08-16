"use client"

import { useCallback, useEffect, useRef } from "react"
import { subscribe } from "@/lib/platform"
import {
  WORKBENCH_PLACE_SESSION_EVENT,
  type WorkbenchPlaceSessionRequest,
} from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { groupOfTab, useTabStore } from "@/stores/tab-store"
import { useWorkbenchStore } from "@/stores/workbench-store"

function isValidRequest(
  value: WorkbenchPlaceSessionRequest
): value is WorkbenchPlaceSessionRequest {
  return (
    Number.isInteger(value?.workbenchId) &&
    value.workbenchId > 0 &&
    Number.isInteger(value?.folderId) &&
    value.folderId > 0 &&
    Number.isInteger(value?.conversationId) &&
    value.conversationId > 0 &&
    typeof value?.agent === "string" &&
    ["tab", "right", "down"].includes(value?.placement)
  )
}

/** Applies one explicit Host Control UI request with the existing Workbench and
 * Pane stores. The backend already persisted membership; this bridge only owns
 * local mounting, focus and the simple VS Code-style split placement. */
export async function applyWorkbenchPlaceSession(
  request: WorkbenchPlaceSessionRequest
) {
  const workspace = useAppWorkspaceStore.getState()
  if (!workspace.folders.some((folder) => folder.id === request.folderId)) {
    await workspace.addFolderToWorkspaceById(request.folderId)
  }

  const workbenches = useWorkbenchStore.getState()
  if (!workbenches.items.some((item) => item.id === request.workbenchId)) {
    await workbenches.hydrate()
  }
  useWorkbenchStore.getState().ensureOpen(request.workbenchId)

  let tabs = useTabStore.getState()
  if (tabs.activeWorkbenchId !== request.workbenchId) {
    await tabs.switchWorkbench(request.workbenchId)
    tabs = useTabStore.getState()
  }

  const existing = tabs.rawTabs.find(
    (tab) =>
      tab.kind === "conversation" &&
      tab.folderId === request.folderId &&
      tab.conversationId === request.conversationId
  )

  if (request.placement === "tab") {
    tabs.openTab(request.folderId, request.conversationId, request.agent, true)
    return
  }

  if (!existing) {
    tabs.openTab(
      request.folderId,
      request.conversationId,
      request.agent,
      true,
      undefined,
      { split: request.placement }
    )
    return
  }

  const sourceGroup = groupOfTab(tabs.groupOf, tabs.groupLayout, existing.id)
  const sourceGroupSize = tabs.rawTabs.filter(
    (tab) => groupOfTab(tabs.groupOf, tabs.groupLayout, tab.id) === sourceGroup
  ).length
  if (sourceGroupSize > 1) {
    tabs.splitTab(existing.id, request.placement, { move: true })
  } else {
    tabs.switchTab(existing.id)
  }
}

/** Serializes placement events so an Agent can arrange several Sessions without
 * Workbench switches racing each other. This is a tiny UI command queue, not a
 * second Session or layout fact source. */
export function HostControlWorkbenchBridge() {
  const foldersHydrated = useAppWorkspaceStore((state) => state.foldersHydrated)
  const tabsHydrated = useTabStore((state) => state.tabsHydrated)
  const pendingRef = useRef<WorkbenchPlaceSessionRequest[]>([])
  const processingRef = useRef(false)

  const drain = useCallback(async () => {
    if (processingRef.current) return
    if (
      !useAppWorkspaceStore.getState().foldersHydrated ||
      !useTabStore.getState().tabsHydrated
    ) {
      return
    }
    processingRef.current = true
    try {
      while (pendingRef.current.length > 0) {
        const request = pendingRef.current.shift()!
        try {
          await applyWorkbenchPlaceSession(request)
        } catch (error) {
          console.error(
            `[HostControlWorkbenchBridge] could not apply ${request.requestId}:`,
            error
          )
        }
      }
    } finally {
      processingRef.current = false
      if (pendingRef.current.length > 0) void drain()
    }
  }, [])

  useEffect(() => {
    void drain()
  }, [foldersHydrated, tabsHydrated, drain])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    void subscribe<WorkbenchPlaceSessionRequest>(
      WORKBENCH_PLACE_SESSION_EVENT,
      (request) => {
        if (!isValidRequest(request)) return
        pendingRef.current.push(request)
        void drain()
      }
    ).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    })
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [drain])

  return null
}
