import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { RoomWorkspace } from "./rooms-page"
import enMessages from "@/i18n/messages/en.json"
import type {
  CollaborationRoomDetail,
  RoomTimeline,
  RoomTimelineEvent,
} from "@/lib/types"

const api = vi.hoisted(() => ({
  getCollaborationRoom: vi.fn(),
  getCollaborationRoomTimeline: vi.fn(),
  markCollaborationRoomSeen: vi.fn(),
  postCollaborationRoomMessage: vi.fn(),
  refreshCatalog: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  getCollaborationRoom: api.getCollaborationRoom,
  getCollaborationRoomTimeline: api.getCollaborationRoomTimeline,
  markCollaborationRoomSeen: api.markCollaborationRoomSeen,
  postCollaborationRoomMessage: api.postCollaborationRoomMessage,
  addCollaborationRoomMembers: vi.fn(),
  removeCollaborationRoomMember: vi.fn(),
  renameCollaborationRoom: vi.fn(),
}))

vi.mock("@/lib/platform", () => ({
  subscribe: vi.fn().mockResolvedValue(() => {}),
}))

vi.mock("@/contexts/tab-context", () => ({
  useTabActions: () => ({ openTab: vi.fn() }),
}))

vi.mock("@/stores/app-workspace-store", () => ({
  useAppWorkspaceStore: (selector: (state: { conversations: [] }) => unknown) =>
    selector({ conversations: [] }),
}))

vi.mock("@/stores/collection-store", () => ({
  useCollectionStore: (
    selector: (state: { items: { id: number; name: string }[] }) => unknown
  ) => selector({ items: [{ id: 7, name: "Research" }] }),
}))

vi.mock("@/stores/room-catalog-store", () => {
  const useRoomCatalogStore = () => ({ rooms: [], hydrated: true })
  useRoomCatalogStore.getState = () => ({ refresh: api.refreshCatalog })
  return { useRoomCatalogStore }
})

const roomId = "rm_plan"

function roomDetail(): CollaborationRoomDetail {
  return {
    id: roomId,
    workbenchId: 1,
    title: "Plan room",
    createdByConversationId: 101,
    members: [
      {
        conversationId: 101,
        title: "Planner",
        agentType: "codex",
        role: "member",
        joinedAt: "2026-08-01T00:00:00.000Z",
      },
    ],
    createdAt: "2026-08-01T00:00:00.000Z",
    updatedAt: "2026-08-18T00:00:00.000Z",
  }
}

function event(overrides: Partial<RoomTimelineEvent> = {}): RoomTimelineEvent {
  return {
    id: "evt-new",
    roomId,
    source: {
      conversationId: 101,
      title: "Planner",
      agentType: "codex",
      folderPath: "/repo",
      backend: "current",
    },
    subject: "newest",
    body: "newest post",
    expectsReply: false,
    urgency: "normal",
    mentionConversationIds: [],
    mentionHuman: false,
    authorKind: "session",
    createdAt: "2026-08-18T12:00:00.000Z",
    ...overrides,
  }
}

function timeline(
  events: RoomTimelineEvent[],
  truncated = false
): RoomTimeline {
  return { roomId, events, truncated }
}

function renderRoom() {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      <RoomWorkspace roomId={roomId} />
    </NextIntlClientProvider>
  )
}

describe("RoomWorkspace", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    api.getCollaborationRoom.mockResolvedValue(roomDetail())
    api.markCollaborationRoomSeen.mockResolvedValue(roomDetail())
    api.postCollaborationRoomMessage.mockResolvedValue({
      eventId: "evt-reply",
      roomId,
      deliveries: [],
      affectedConversationIds: [101],
      deduplicated: false,
    })
  })

  it("pages older posts without marking the room seen again", async () => {
    api.getCollaborationRoomTimeline.mockImplementation(
      async (_id: string, _limit?: number, beforeEventId?: string | null) => {
        if (beforeEventId === "evt-new") {
          return timeline(
            [event({ id: "evt-old", body: "older post" })],
            false
          )
        }
        return timeline([event()], true)
      }
    )

    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Load older posts" }))

    expect(await screen.findByText("older post")).toBeTruthy()
    expect(screen.getByText("newest post")).toBeTruthy()
    expect(
      screen.queryByRole("button", { name: "Load older posts" })
    ).toBeNull()

    expect(api.getCollaborationRoomTimeline).toHaveBeenNthCalledWith(
      2,
      roomId,
      undefined,
      "evt-new"
    )
    expect(api.markCollaborationRoomSeen).toHaveBeenCalledTimes(1)
    expect(api.markCollaborationRoomSeen).toHaveBeenCalledWith(roomId)
  })

  it("does not show owner ranks or an @human chip in the host composer", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    expect(screen.getByText("Uncategorized · 1 members")).toBeTruthy()
    expect(screen.queryByText("You host this room")).toBeNull()
    expect(screen.queryByText("owner")).toBeNull()
    expect(screen.queryByRole("button", { name: "@human" })).toBeNull()
    expect(screen.getByRole("button", { name: "@all" })).toBeTruthy()
  })

  it("shows the Collection name instead of a Session folder breadcrumb", async () => {
    api.getCollaborationRoom.mockResolvedValue({
      ...roomDetail(),
      collectionId: 7,
    })
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))
    renderRoom()

    expect(await screen.findByText("Research · 1 members")).toBeTruthy()
    expect(screen.queryByText("Uncategorized · 1 members")).toBeNull()
  })

  it("posts a reply against the selected event", async () => {
    api.getCollaborationRoomTimeline.mockResolvedValue(timeline([event()]))

    renderRoom()

    expect(await screen.findByText("newest post")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Reply" }))
    expect(screen.getByText("Replying to Planner")).toBeTruthy()

    fireEvent.change(screen.getByPlaceholderText(/Write to the room/), {
      target: { value: "ack" },
    })
    fireEvent.click(screen.getByRole("button", { name: "Send" }))

    await waitFor(() => {
      expect(api.postCollaborationRoomMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          roomId,
          body: "ack",
          authorKind: "human",
          replyToEventId: "evt-new",
          invocationPolicy: "store_only",
          expectsReply: false,
        })
      )
    })
  })
})
