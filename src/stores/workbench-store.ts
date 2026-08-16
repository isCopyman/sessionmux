import { create } from "zustand"
import {
  createWorkbench as createWorkbenchApi,
  deleteWorkbench as deleteWorkbenchApi,
  duplicateWorkbench as duplicateWorkbenchApi,
  listWorkbenches,
  reorderWorkbenches as reorderWorkbenchesApi,
  renameWorkbench as renameWorkbenchApi,
  setWorkbenchPinned as setWorkbenchPinnedApi,
} from "@/lib/api"
import type { WorkbenchInfo } from "@/lib/types"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { duplicateWorkbenchLocalState, useTabStore } from "@/stores/tab-store"
import {
  closeWorkbenchWindowTab,
  loadWorkbenchWindowTabs,
  openWorkbenchWindowTab,
  saveWorkbenchWindowTabs,
} from "@/lib/workbench-window-tabs"

interface WorkbenchStoreState {
  items: WorkbenchInfo[]
  /** Workbench views mounted in this app window; saved Workbenches stay in items. */
  openIds: number[]
  /** Most-recently closed local views, newest first. */
  recentlyClosedIds: number[]
  hydrated: boolean
  loading: boolean
  refreshQueued: boolean
  hydrate: () => Promise<void>
  createAndSwitch: (name: string) => Promise<WorkbenchInfo>
  /** Persist a Workbench without switching this window to it. */
  createOnly: (name: string) => Promise<WorkbenchInfo>
  duplicateAndSwitch: (id: number, name: string) => Promise<WorkbenchInfo>
  rename: (id: number, name: string) => Promise<void>
  setPinned: (id: number, isPinned: boolean) => Promise<void>
  previewOrder: (items: WorkbenchInfo[]) => void
  persistOrder: () => Promise<void>
  ensureOpen: (id: number) => void
  closeView: (id: number) => Promise<void>
  reopenAndSwitch: (id: number) => Promise<void>
  remove: (id: number) => Promise<void>
}

function ordered(items: WorkbenchInfo[]) {
  return [...items].sort(
    (a, b) =>
      Number(b.is_pinned) - Number(a.is_pinned) ||
      a.position - b.position ||
      a.id - b.id
  )
}

function seedDraftIfEmpty() {
  const tabs = useTabStore.getState()
  if (tabs.rawTabs.length > 0) return
  const workspace = useAppWorkspaceStore.getState()
  const folder = workspace.folders.find(
    (item) => item.id === workspace.activeFolderId
  )
  if (folder) tabs.openNewConversationTab(folder.id, folder.path)
  else tabs.openChatModeTab()
}

export const useWorkbenchStore = create<WorkbenchStoreState>()((set, get) => ({
  items: [],
  openIds: [],
  recentlyClosedIds: [],
  hydrated: false,
  loading: false,
  refreshQueued: false,

  hydrate: async () => {
    if (get().loading) {
      set({ refreshQueued: true })
      return
    }
    set({ loading: true, refreshQueued: false })
    try {
      do {
        set({ refreshQueued: false })
        const items = ordered(await listWorkbenches())
        const windowTabs = loadWorkbenchWindowTabs(
          items.map((item) => item.id),
          useTabStore.getState().activeWorkbenchId
        )
        saveWorkbenchWindowTabs(windowTabs)
        set({ items, ...windowTabs, hydrated: true })
      } while (get().refreshQueued)
      const items = get().items
      const activeId = useTabStore.getState().activeWorkbenchId
      const stillOpen = get().openIds.includes(activeId)
      if (
        items.length > 0 &&
        (!items.some((item) => item.id === activeId) || !stillOpen)
      ) {
        const fallback =
          items.find((item) => item.id === 1) ??
          items.find((item) => get().openIds.includes(item.id)) ??
          items[0]
        if (fallback && fallback.id !== activeId) {
          await useTabStore.getState().switchWorkbench(fallback.id)
        }
      }
    } finally {
      set({ loading: false, refreshQueued: false })
    }
  },

  createOnly: async (name) => {
    const created = await createWorkbenchApi(name)
    set({ items: ordered([...get().items, created]) })
    return created
  },

  createAndSwitch: async (name) => {
    const created = await get().createOnly(name)
    await useTabStore.getState().switchWorkbench(created.id)
    get().ensureOpen(created.id)

    // A brand-new workbench should be immediately usable. Seed one draft from
    // the currently focused execution folder (or folderless chat when none is
    // available); drafts stay device-local until the first message binds them.
    seedDraftIfEmpty()
    return created
  },

  duplicateAndSwitch: async (id, name) => {
    const tabs = useTabStore.getState()
    if (tabs.activeWorkbenchId === id) await tabs.flushActiveWorkbench()
    const created = await duplicateWorkbenchApi(id, name)
    duplicateWorkbenchLocalState(id, created.id)
    set({ items: ordered([...get().items, created]) })
    await useTabStore.getState().switchWorkbench(created.id)
    get().ensureOpen(created.id)
    seedDraftIfEmpty()
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

  setPinned: async (id, isPinned) => {
    const updated = await setWorkbenchPinnedApi(id, isPinned)
    set({
      items: ordered(
        get().items.map((item) => (item.id === updated.id ? updated : item))
      ),
    })
  },

  previewOrder: (items) => {
    const normalized = [
      ...items.filter((item) => item.is_pinned),
      ...items.filter((item) => !item.is_pinned),
    ]
    set({
      items: normalized.map((item, position) => ({ ...item, position })),
    })
  },

  persistOrder: async () => {
    const ids = get().items.map((item) => item.id)
    try {
      set({ items: ordered(await reorderWorkbenchesApi(ids)) })
    } catch (error) {
      await get().hydrate()
      throw error
    }
  },

  ensureOpen: (id) => {
    if (!get().items.some((item) => item.id === id)) return
    const next = openWorkbenchWindowTab(get(), id)
    if (
      next.openIds === get().openIds &&
      next.recentlyClosedIds.length === get().recentlyClosedIds.length
    ) {
      return
    }
    saveWorkbenchWindowTabs(next)
    set(next)
  },

  closeView: async (id) => {
    const current = get()
    if (!current.items.some((item) => item.id === id)) return
    // A sidebar-triggered switch can expose the active tab one render before
    // the effect records it in openIds. Treat that active surface as open so a
    // very fast close click still behaves deterministically.
    const mounted = openWorkbenchWindowTab(current, id)
    const next = closeWorkbenchWindowTab(mounted, id)
    if (next === mounted) return

    if (useTabStore.getState().activeWorkbenchId === id) {
      const openItems = current.items.filter((item) =>
        current.openIds.includes(item.id)
      )
      const index = openItems.findIndex((item) => item.id === id)
      const fallback = openItems[index + 1] ?? openItems[index - 1]
      if (!fallback) return
      await useTabStore.getState().switchWorkbench(fallback.id)
    }

    saveWorkbenchWindowTabs(next)
    set(next)
  },

  reopenAndSwitch: async (id) => {
    get().ensureOpen(id)
    await useTabStore.getState().switchWorkbench(id)
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
    const remainingItems = current.filter((item) => item.id !== id)
    const nextWindowTabs = {
      openIds: get().openIds.filter((candidate) => candidate !== id),
      recentlyClosedIds: get().recentlyClosedIds.filter(
        (candidate) => candidate !== id
      ),
    }
    if (nextWindowTabs.openIds.length === 0 && remainingItems[0]) {
      nextWindowTabs.openIds.push(remainingItems[0].id)
    }
    saveWorkbenchWindowTabs(nextWindowTabs)
    set({ items: remainingItems, ...nextWindowTabs })
  },
}))
