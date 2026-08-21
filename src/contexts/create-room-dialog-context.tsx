"use client"

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from "react"

interface CreateRoomDialogContextValue {
  open: boolean
  setOpen: Dispatch<SetStateAction<boolean>>
  /** Canonical Path to limit initiator candidates. Null = every live Session. */
  folderScopeId: number | null
  openForFolder: (folderId: number) => void
}

const CreateRoomDialogContext =
  createContext<CreateRoomDialogContextValue | null>(null)

/**
 * Shared open-state for the create-room dialog, mirroring
 * `search-dialog-context`.
 *
 * The dialog itself is owned by `WorkspaceChromeController` (always mounted on
 * desktop + mobile). Triggers live wherever they make sense — the sidebar row,
 * the new-conversation welcome page — and the sidebar in particular unmounts
 * when collapsed, so the boolean cannot live there or the welcome-page entry
 * would be dead whenever the sidebar is hidden.
 *
 * `openForFolder` is the classification-tree entry: same dialog, candidates
 * narrowed to that Path. `setOpen(true)` stays the unscoped global open.
 * Closing always clears the scope so the next global open is not sticky.
 */
export function useCreateRoomDialog() {
  const ctx = useContext(CreateRoomDialogContext)
  if (!ctx) {
    throw new Error(
      "useCreateRoomDialog must be used within CreateRoomDialogProvider"
    )
  }
  return ctx
}

export function CreateRoomDialogProvider({
  children,
}: {
  children: ReactNode
}) {
  const [open, setOpenState] = useState(false)
  const [folderScopeId, setFolderScopeId] = useState<number | null>(null)

  const setOpen = useCallback<Dispatch<SetStateAction<boolean>>>((action) => {
    if (typeof action === "function") {
      setOpenState((prev) => {
        const next = action(prev)
        if (!next) setFolderScopeId(null)
        return next
      })
      return
    }
    // Boolean form is the global open/close path (sidebar, welcome page,
    // dialog onOpenChange). Never inherit a leftover folder scope.
    setFolderScopeId(null)
    setOpenState(action)
  }, [])

  const openForFolder = useCallback((folderId: number) => {
    setFolderScopeId(folderId)
    setOpenState(true)
  }, [])

  const value = useMemo<CreateRoomDialogContextValue>(
    () => ({ open, setOpen, folderScopeId, openForFolder }),
    [open, setOpen, folderScopeId, openForFolder]
  )

  return (
    <CreateRoomDialogContext.Provider value={value}>
      {children}
    </CreateRoomDialogContext.Provider>
  )
}
