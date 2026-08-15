import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { CollectionTree } from "./collection-tree"
import enMessages from "@/i18n/messages/en.json"
import type { DbConversationSummary } from "@/lib/types"

const h = vi.hoisted(() => ({
  create: vi.fn(),
  rename: vi.fn(),
  move: vi.fn(),
  remove: vi.fn(),
  hydrate: vi.fn(),
  listRefs: vi.fn(),
  conversations: [
    {
      id: 101,
      folder_id: 7,
      title: "Evidence review",
      title_locked: true,
      agent_type: "codex",
      status: "in_progress",
      kind: "regular",
      model: null,
      git_branch: null,
      external_id: "session-101",
      message_count: 3,
      child_count: 0,
      created_at: "2026-06-03T00:00:00.000Z",
      updated_at: "2026-06-03T00:00:00.000Z",
      archived_at: null,
      pinned_at: null,
    },
    {
      id: 102,
      folder_id: 7,
      title: "Loose notes",
      title_locked: true,
      agent_type: "claude",
      status: "in_progress",
      kind: "regular",
      model: null,
      git_branch: null,
      external_id: "session-102",
      message_count: 2,
      child_count: 0,
      created_at: "2026-06-02T00:00:00.000Z",
      updated_at: "2026-06-02T00:00:00.000Z",
      archived_at: null,
      pinned_at: null,
    },
  ],
  items: [
    {
      id: 10,
      root_folder_id: 7,
      parent_id: null,
      name: "Research",
      position: 0,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    },
    {
      id: 11,
      root_folder_id: 7,
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

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: unknown) => unknown) =>
    selector({
      conversations: h.conversations,
      activeFolderId: 7,
      allFolders: [
        {
          id: 7,
          name: "project",
          path: "/tmp/project",
          parent_id: null,
          kind: "regular",
        },
      ],
    }),
}))

vi.mock("@/lib/api", () => ({
  listConversationCollectionRefs: h.listRefs,
}))

function renderTree(
  onOpenScope = vi.fn(),
  options: {
    showSessions?: boolean
    onOpenSession?: (session: DbConversationSummary) => void
  } = {}
) {
  render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <CollectionTree onOpenScope={onOpenScope} {...options} />
    </NextIntlClientProvider>
  )
  return { user: userEvent.setup(), onOpenScope }
}

describe("CollectionTree", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.create.mockResolvedValue({
      id: 12,
      root_folder_id: 7,
      parent_id: null,
      name: "Writing",
      position: 1,
      created_at: "2026-06-01T00:00:00.000Z",
      updated_at: "2026-06-01T00:00:00.000Z",
    })
    h.listRefs.mockResolvedValue([{ conversation_id: 101, collection_id: 11 }])
  })

  it("opens nested Collections as Session Center scopes", async () => {
    const { user, onOpenScope } = renderTree()
    await user.click(screen.getByRole("button", { name: "Expand collection" }))
    await user.click(screen.getByTitle("Sources"))

    expect(onOpenScope).toHaveBeenCalledWith(11)
  })

  it("creates a top-level Collection without touching execution folders", async () => {
    const { user, onOpenScope } = renderTree()
    await user.click(screen.getByRole("button", { name: "New collection" }))
    await user.type(screen.getByPlaceholderText("Collection name"), "Writing")
    await user.click(screen.getByRole("button", { name: "Confirm" }))

    expect(h.create).toHaveBeenCalledWith("Writing", null, 7)
    expect(onOpenScope).toHaveBeenCalledWith(12)
  })

  it("expands Collections into inline Sessions and keeps Unclassified visible", async () => {
    const onOpenSession = vi.fn()
    const { user } = renderTree(vi.fn(), {
      showSessions: true,
      onOpenSession,
    })

    expect(await screen.findByText("Loose notes")).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "Research" }))
    await user.click(screen.getByTitle("Sources"))
    await user.click(await screen.findByText("Evidence review"))

    expect(onOpenSession).toHaveBeenCalledWith(
      expect.objectContaining({ id: 101, title: "Evidence review" })
    )
  })
})
