"use client"

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react"

import { ConversationManageDialog } from "@/components/conversations/conversation-manage-dialog"

/** The scope a caller can pre-select: a Collection, its Unclassified bucket, or
 *  the whole workspace (`null`). */
export type SessionCenterCollection = number | "unclassified" | null

/** The collaboration facet an entry point can pre-select. A narrow slice of the
 *  dialog's own `CollaborationFilter` — only the facets something in the app
 *  actually links to. */
export type SessionCenterCollaborationFilter = "all" | "needs_reply"

export interface OpenSessionCenterOptions {
  collabFilter?: SessionCenterCollaborationFilter
  collection?: SessionCenterCollection
}

interface SessionCenterContextValue {
  openSessionCenter: (options?: OpenSessionCenterOptions) => void
  /** Bumped every time the Session Center closes. The sidebar's Collection tree
   *  keys its refetch off this: the dialog can move Sessions between
   *  Collections, so the tree behind it is stale the moment it closes. */
  closedRevision: number
}

const SessionCenterContext = createContext<SessionCenterContextValue | null>(
  null
)

/**
 * Shared entry point for the Session Center (`ConversationManageDialog`).
 *
 * Same lifted-state idiom as `search-dialog-context`, one step further: the
 * provider owns the dialog too. The sidebar used to mount it, but the sidebar
 * unmounts when collapsed — and the Session Center now has entry points outside
 * the sidebar's tree (its needs-reply badge, the Collection tree, and the
 * command palette next). Callers get one function and never touch the state.
 */
export function useSessionCenter() {
  const ctx = useContext(SessionCenterContext)
  if (!ctx) {
    throw new Error(
      "useSessionCenter must be used within SessionCenterProvider"
    )
  }
  return ctx
}

export function SessionCenterProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false)
  const [collection, setCollection] = useState<SessionCenterCollection>(null)
  const [collabFilter, setCollabFilter] =
    useState<SessionCenterCollaborationFilter>("all")
  const [closedRevision, setClosedRevision] = useState(0)

  const openSessionCenter = useCallback(
    (options?: OpenSessionCenterOptions) => {
      // Omitted options reset rather than inherit: every entry point states
      // the scope it wants, so the last visit's filter can't leak into this
      // one.
      setCollection(options?.collection ?? null)
      setCollabFilter(options?.collabFilter ?? "all")
      setOpen(true)
    },
    []
  )

  const handleOpenChange = useCallback((next: boolean) => {
    setOpen(next)
    if (!next) setClosedRevision((current) => current + 1)
  }, [])

  const value = useMemo<SessionCenterContextValue>(
    () => ({ openSessionCenter, closedRevision }),
    [openSessionCenter, closedRevision]
  )

  return (
    <SessionCenterContext.Provider value={value}>
      {children}
      {/* Mounted only while open, so the `initial*` props are read fresh on
          every visit instead of sticking at the first one. */}
      {open ? (
        <ConversationManageDialog
          open
          onOpenChange={handleOpenChange}
          folderId={null}
          initialCollection={collection}
          initialCollaborationFilter={collabFilter}
        />
      ) : null}
    </SessionCenterContext.Provider>
  )
}
