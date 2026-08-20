import { beforeEach, describe, expect, it, vi } from "vitest"

import type { CollectionInfo } from "@/lib/types"
import { useCollectionStore } from "./collection-store"

const h = vi.hoisted(() => ({
  list: vi.fn(),
  create: vi.fn(),
  rename: vi.fn(),
  move: vi.fn(),
  place: vi.fn(),
  remove: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  listCollections: h.list,
  createCollection: h.create,
  renameCollection: h.rename,
  moveCollection: h.move,
  placeCollection: h.place,
  deleteCollection: h.remove,
}))

function collection(
  id: number,
  name: string,
  parent_id: number | null = null
): CollectionInfo {
  return {
    id,
    parent_id,
    name,
    position: 0,
    created_at: "2026-06-01T00:00:00.000Z",
    updated_at: "2026-06-01T00:00:00.000Z",
  }
}

describe("collection store", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useCollectionStore.setState({
      items: [],
      hydrated: false,
      loading: false,
      refreshQueued: false,
    })
  })

  it("hydrates and keeps the semantic hierarchy independent of folders", async () => {
    h.list.mockResolvedValue([
      { ...collection(2, "Sources", 1), position: 0 },
      { ...collection(1, "Research"), position: 0 },
    ])

    await useCollectionStore.getState().hydrate()

    expect(useCollectionStore.getState().items).toEqual([
      collection(1, "Research"),
      collection(2, "Sources", 1),
    ])
  })

  it("runs one trailing refresh when a forced invalidation arrives mid-load", async () => {
    let finishFirst!: (items: CollectionInfo[]) => void
    h.list
      .mockImplementationOnce(
        () =>
          new Promise<CollectionInfo[]>((resolve) => (finishFirst = resolve))
      )
      .mockResolvedValueOnce([collection(2, "Latest")])

    const first = useCollectionStore.getState().hydrate(true)
    await useCollectionStore.getState().hydrate(true)
    finishFirst([collection(1, "Stale")])
    await first

    expect(h.list).toHaveBeenCalledTimes(2)
    expect(useCollectionStore.getState().items).toEqual([
      collection(2, "Latest"),
    ])
  })

  it("reparents children locally when a Collection is deleted", async () => {
    useCollectionStore.setState({
      items: [
        collection(1, "Research"),
        collection(2, "Sources", 1),
        collection(3, "Papers", 2),
      ],
      hydrated: true,
    })
    h.remove.mockResolvedValue(undefined)

    await useCollectionStore.getState().remove(2)

    expect(h.remove).toHaveBeenCalledWith(2)
    expect(useCollectionStore.getState().items).toEqual([
      collection(1, "Research"),
      collection(3, "Papers", 1),
    ])
  })

  it("replaces the tree with the authoritative placement snapshot", async () => {
    const moved = [
      { ...collection(2, "Second"), position: 0 },
      { ...collection(1, "First"), position: 1 },
    ]
    h.place.mockResolvedValue(moved)

    await useCollectionStore.getState().place(2, null, 0)

    expect(h.place).toHaveBeenCalledWith(2, null, 0)
    expect(useCollectionStore.getState().items).toEqual(moved)
  })
})
