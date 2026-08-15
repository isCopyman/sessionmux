import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { CollectionTree } from "./collection-tree"
import enMessages from "@/i18n/messages/en.json"

const h = vi.hoisted(() => ({
  create: vi.fn(),
  rename: vi.fn(),
  move: vi.fn(),
  remove: vi.fn(),
  hydrate: vi.fn(),
  items: [
    {
      id: 10,
      parent_id: null,
      name: "Research",
      position: 0,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    },
    {
      id: 11,
      parent_id: 10,
      name: "Sources",
      position: 0,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    },
  ],
}))

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock("@/stores/collection-store", () => ({
  useCollectionStore: (selector: (state: unknown) => unknown) =>
    selector({
      items: h.items,
      hydrated: true,
      loading: false,
      hydrate: h.hydrate,
      create: h.create,
      rename: h.rename,
      move: h.move,
      remove: h.remove,
    }),
}))

function renderTree(onOpenScope = vi.fn()) {
  render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <CollectionTree onOpenScope={onOpenScope} />
    </NextIntlClientProvider>
  )
  return { user: userEvent.setup(), onOpenScope }
}

describe("CollectionTree", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.create.mockResolvedValue({
      id: 12,
      parent_id: null,
      name: "Writing",
      position: 1,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    })
  })

  it("opens nested Collections as Session Center scopes", async () => {
    const { user, onOpenScope } = renderTree()
    await user.click(screen.getByRole("button", { name: "Expand collection" }))
    await user.click(screen.getByRole("button", { name: "Sources" }))

    expect(onOpenScope).toHaveBeenCalledWith(11)
  })

  it("creates a top-level Collection without touching execution folders", async () => {
    const { user, onOpenScope } = renderTree()
    await user.click(screen.getByRole("button", { name: "New collection" }))
    await user.type(screen.getByPlaceholderText("Collection name"), "Writing")
    await user.click(screen.getByRole("button", { name: "Confirm" }))

    expect(h.create).toHaveBeenCalledWith("Writing", null)
    expect(onOpenScope).toHaveBeenCalledWith(12)
  })
})
