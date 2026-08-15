import { beforeEach, describe, expect, it, vi } from "vitest"

import type { CollectionInfo } from "@/lib/types"
import { useCollectionStore } from "./collection-store"

const h = vi.hoisted(() => ({
  list: vi.fn(),
  create: vi.fn(),
  rename: vi.fn(),
  move: vi.fn(),
  remove: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  listCollections: h.list,
  createCollection: h.create,
  renameCollection: h.rename,
  moveCollection: h.move,
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
})
