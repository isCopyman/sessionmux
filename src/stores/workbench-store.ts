import { create } from "zustand"
import {
  createWorkbench as createWorkbenchApi,
  deleteWorkbench as deleteWorkbenchApi,
  listWorkbenches,
  renameWorkbench as renameWorkbenchApi,
} from "@/lib/api"
import type { WorkbenchInfo } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { useTabStore } from "@/stores/tab-store"

interface WorkbenchStoreState {
  items: WorkbenchInfo[]
  hydrated: boolean
  loading: boolean
  hydrate: () => Promise<void>
  createAndSwitch: (name: string) => Promise<WorkbenchInfo>
  rename: (id: number, name: string) => Promise<void>
  remove: (id: number) => Promise<void>
}

function ordered(items: WorkbenchInfo[]) {
  return [...items].sort((a, b) => a.position - b.position || a.id - b.id)
}

export const useWorkbenchStore = create<WorkbenchStoreState>()((set, get) => ({
  items: [],
  hydrated: false,
  loading: false,

  hydrate: async () => {
    if (get().loading) return
    set({ loading: true })
    try {
      set({ items: ordered(await listWorkbenches()), hydrated: true })
    } finally {
      set({ loading: false })
    }
  },

  createAndSwitch: async (name) => {
    const created = await createWorkbenchApi(name)
    set({ items: ordered([...get().items, created]) })
    await useTabStore.getState().switchWorkbench(created.id)

    // A brand-new workbench should be immediately usable. Seed one draft from
    // the currently focused execution folder (or folderless chat when none is
    // available); drafts stay device-local until the first message binds them.
    const tabs = useTabStore.getState()
    if (tabs.rawTabs.length === 0) {
      const workspace = useAppWorkspaceStore.getState()
      const folder = workspace.folders.find(
        (item) => item.id === workspace.activeFolderId
      )
      if (folder) tabs.openNewConversationTab(folder.id, folder.path)
      else tabs.openChatModeTab()
    }
    return created
  },

  rename: async (id, name) => {
    const updated = await renameWorkbenchApi(id, name)
    set({
      items: ordered(
        get().items.map((item) => (item.id === updated.id ? updated : item))
      ),
    })
  },

  remove: async (id) => {
    const current = get().items
    if (current.length <= 1)
      throw new Error("The last workbench cannot be deleted")
    if (useTabStore.getState().activeWorkbenchId === id) {
      const fallback = current.find((item) => item.id !== id)
      if (!fallback) throw new Error("No fallback workbench")
      await useTabStore.getState().switchWorkbench(fallback.id)
    }
    await deleteWorkbenchApi(id)
    set({ items: current.filter((item) => item.id !== id) })
  },
}))
