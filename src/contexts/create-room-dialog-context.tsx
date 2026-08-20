"use client"

import {
  createContext,
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
  const [open, setOpen] = useState(false)
  const value = useMemo<CreateRoomDialogContextValue>(
    () => ({ open, setOpen }),
    [open]
  )

  return (
    <CreateRoomDialogContext.Provider value={value}>
      {children}
    </CreateRoomDialogContext.Provider>
  )
}
