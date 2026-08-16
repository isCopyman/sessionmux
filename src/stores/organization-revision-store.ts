import { create } from "zustand"

interface OrganizationRevisionState {
  revision: number
  invalidate: () => void
}

/** Low-cost invalidation clock for headless Collection/Workbench changes.
 * Domain truth remains in SQLite; consumers use this only to refetch it. */
export const useOrganizationRevisionStore = create<OrganizationRevisionState>()(
  (set) => ({
    revision: 0,
    invalidate: () => set((state) => ({ revision: state.revision + 1 })),
  })
)
