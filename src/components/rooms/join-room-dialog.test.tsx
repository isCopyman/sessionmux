import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { NextIntlClientProvider } from "next-intl"
import { toast } from "sonner"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { JoinRoomDialog } from "./join-room-dialog"
import enMessages from "@/i18n/messages/en.json"
import type { CollaborationRoomSummary } from "@/lib/types"

const h = vi.hoisted(() => ({
  rooms: [] as CollaborationRoomSummary[],
  collections: [] as { id: number; name: string }[],
  folders: [] as { id: number; name: string }[],
  refresh: vi.fn(async () => {}),
  addCollaborationRoomMembers: vi.fn(),
}))

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock("@/lib/api", () => ({
  addCollaborationRoomMembers: h.addCollaborationRoomMembers,
}))

vi.mock("@/stores/room-catalog-store", () => {
  const useRoomCatalogStore = (
    selector: (state: {
      rooms: CollaborationRoomSummary[]
      hydrated: boolean
      refresh: () => Promise<void>
    }) => unknown
  ) =>
    selector({
      rooms: h.rooms,
      hydrated: true,
      refresh: h.refresh,
    })
  useRoomCatalogStore.getState = () => ({
    rooms: h.rooms,
    hydrated: true,
    refresh: h.refresh,
  })
  return { useRoomCatalogStore }
})

vi.mock("@/stores/collection-store", () => ({
  useCollectionStore: (
    selector: (state: { items: { id: number; name: string }[] }) => unknown
  ) => selector({ items: h.collections }),
}))

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (
    selector: (state: { allFolders: { id: number; name: string }[] }) => unknown
  ) => selector({ allFolders: h.folders }),
}))

function room(
  over: Partial<CollaborationRoomSummary> & { id: string; title: string }
): CollaborationRoomSummary {
  return {
    workbenchId: 1,
    createdByConversationId: 1,
    collectionId: null,
    rootFolderId: null,
    memberCount: 2,
    additionalPathCount: 0,
    unreadCount: 0,
    mentionUnreadCount: 0,
    needsReplyCount: 0,
    awaitingReplyCount: 0,
    lastEventAt: null,
    createdAt: "2026-06-10T11:00:00.000Z",
    updatedAt: "2026-06-10T11:00:00.000Z",
    ...over,
  }
}

function renderDialog(
  onOpenChange: (open: boolean) => void = vi.fn(),
  conversationId = 42
) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <JoinRoomDialog
        open
        onOpenChange={onOpenChange}
        conversationId={conversationId}
      />
    </NextIntlClientProvider>
  )
}

describe("JoinRoomDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    h.rooms.splice(
      0,
      h.rooms.length,
      room({
        id: "rm_alpha",
        title: "Alpha room",
        memberCount: 3,
        collectionId: 7,
        rootFolderId: 11,
      }),
      room({
        id: "rm_beta",
        title: "Beta room",
        memberCount: 1,
      }),
      room({
        id: "rm_standup_ops",
        title: "Standup",
        collectionId: 8,
        rootFolderId: 12,
      }),
      room({
        id: "rm_standup_eng",
        title: "Standup",
        collectionId: 9,
        rootFolderId: 13,
      })
    )
    h.collections.splice(
      0,
      h.collections.length,
      { id: 7, name: "Ops" },
      { id: 8, name: "Daily ops" },
      { id: 9, name: "Engineering" }
    )
    h.folders.splice(
      0,
      h.folders.length,
      { id: 11, name: "ops-repo" },
      { id: 12, name: "ops-daily" },
      { id: 13, name: "eng-core" }
    )
    h.addCollaborationRoomMembers.mockResolvedValue({ id: "rm_alpha" })
  })

  it("lists catalog rooms with member count and collection/folder labels", () => {
    renderDialog()

    expect(screen.getByRole("heading", { name: "Join a room" })).toBeTruthy()
    expect(screen.getByRole("radio", { name: /Alpha room/ })).toBeTruthy()
    expect(screen.getByText("3 members · Ops · ops-repo")).toBeTruthy()
    expect(screen.getByRole("radio", { name: /Beta room/ })).toBeTruthy()
    expect(screen.getByText("1 members")).toBeTruthy()
    expect(
      screen.getByRole("radio", { name: /Daily ops · ops-daily/ })
    ).toBeTruthy()
    expect(
      screen.getByRole("radio", { name: /Engineering · eng-core/ })
    ).toBeTruthy()
    expect(screen.getByRole("button", { name: "Join room" })).toBeDisabled()
  })

  it("filters the list by title", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Search rooms"), "beta")

    expect(screen.getByRole("radio", { name: /Beta room/ })).toBeTruthy()
    expect(screen.queryByRole("radio", { name: /Alpha room/ })).toBeNull()
    expect(screen.queryByRole("radio", { name: /Standup/ })).toBeNull()
  })

  it("shows empty copy when nothing matches", async () => {
    const user = userEvent.setup()
    renderDialog()

    await user.type(screen.getByLabelText("Search rooms"), "zzzz")

    expect(screen.getByText("No rooms to join.")).toBeTruthy()
    expect(screen.queryByRole("radio")).toBeNull()
  })

  it("shows empty copy when the catalog has no rooms", () => {
    h.rooms.splice(0, h.rooms.length)
    renderDialog()

    expect(screen.getByText("No rooms to join.")).toBeTruthy()
    expect(screen.queryByRole("radio")).toBeNull()
  })

  it("joins the selected room with this Session id", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    renderDialog(onOpenChange, 42)

    await user.click(screen.getByRole("radio", { name: /Beta room/ }))
    const join = screen.getByRole("button", { name: "Join room" })
    expect(join).toBeEnabled()
    await user.click(join)

    await waitFor(() => {
      expect(h.addCollaborationRoomMembers).toHaveBeenCalledWith({
        roomId: "rm_beta",
        conversationIds: [42],
      })
    })
    expect(toast.success).toHaveBeenCalledWith("Joined Beta room")
    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(h.refresh).toHaveBeenCalled()
  })

  it("toasts the backend error when join fails", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    h.addCollaborationRoomMembers.mockRejectedValueOnce(
      new Error("already a member")
    )
    renderDialog(onOpenChange, 42)

    await user.click(screen.getByRole("radio", { name: /Alpha room/ }))
    await user.click(screen.getByRole("button", { name: "Join room" }))

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith("already a member")
    })
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
    expect(h.refresh).not.toHaveBeenCalled()
  })
})
