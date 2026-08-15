import { create } from "zustand"
import {
  createCollection as createCollectionApi,
  deleteCollection as deleteCollectionApi,
  listCollections,
  moveCollection as moveCollectionApi,
  renameCollection as renameCollectionApi,
} from "@/lib/api"
import type { CollectionInfo } from "@/lib/types"

interface CollectionStoreState {
  items: CollectionInfo[]
  hydrated: boolean
  loading: boolean
  hydrate: (force?: boolean) => Promise<void>
  create: (
    name: string,
    parentId?: number | null,
    rootFolderId?: number | null
  ) => Promise<CollectionInfo>
  rename: (id: number, name: string) => Promise<void>
  move: (id: number, parentId?: number | null) => Promise<void>
  remove: (id: number) => Promise<void>
}

function ordered(items: CollectionInfo[]) {
  return [...items].sort(
    (a, b) =>
      (a.root_folder_id ?? -1) - (b.root_folder_id ?? -1) ||
      (a.parent_id ?? -1) - (b.parent_id ?? -1) ||
      a.position - b.position ||
      a.id - b.id
  )
}

export const useCollectionStore = create<CollectionStoreState>()(
  (set, get) => ({
    items: [],
    hydrated: false,
    loading: false,

    hydrate: async (force = false) => {
      if (get().loading || (get().hydrated && !force)) return
      set({ loading: true })
      try {
        set({ items: ordered(await listCollections()), hydrated: true })
      } finally {
        set({ loading: false })
      }
    },

    create: async (name, parentId = null, rootFolderId = null) => {
      const created = await createCollectionApi(name, parentId, rootFolderId)
      set({ items: ordered([...get().items, created]) })
      return created
    },

    rename: async (id, name) => {
      const updated = await renameCollectionApi(id, name)
      set({
        items: ordered(
          get().items.map((item) => (item.id === id ? updated : item))
        ),
      })
    },

    move: async (id, parentId = null) => {
      const updated = await moveCollectionApi(id, parentId)
      set({
        items: ordered(
          get().items.map((item) => (item.id === id ? updated : item))
        ),
      })
    },

    remove: async (id) => {
      const removed = get().items.find((item) => item.id === id)
      await deleteCollectionApi(id)
      set({
        items: ordered(
          get()
            .items.filter((item) => item.id !== id)
            .map((item) =>
              item.parent_id === id
                ? { ...item, parent_id: removed?.parent_id ?? null }
                : item
            )
        ),
      })
    },
  })
)
