import { type ReactElement } from "react"
import { fireEvent, render, screen } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"

import enMessages from "@/i18n/messages/en.json"
import type { CollaborationRoomSummary } from "@/lib/types"

import { SessionMailCard } from "./session-mail-card"
import { SessionRoomChip } from "./session-room-chip"

const h = vi.hoisted(() => {
  const refresh = vi.fn()
  return {
    openRoom: vi.fn(),
    ensure: vi.fn(),
    refresh,
    state: { rooms: [] as unknown[], hydrated: true, refresh },
  }
})

vi.mock("@/lib/open-room", () => ({ useOpenRoom: () => h.openRoom }))
vi.mock("@/stores/room-catalog-store", () => {
  const useRoomCatalogStore = (selector: (value: typeof h.state) => unknown) =>
    selector(h.state)
  useRoomCatalogStore.getState = () => h.state
  useRoomCatalogStore.subscribe = vi.fn(() => () => {})
  return { useRoomCatalogStore, ensureRoomCatalogSubscription: h.ensure }
})

const ROOM_ID = "rm_1f9c2b7a-4d1e-4a55-9c0b-77f0c1a2b3d4"

function room(
  overrides: Partial<CollaborationRoomSummary> = {}
): CollaborationRoomSummary {
  return {
    id: ROOM_ID,
    workbenchId: 1,
    title: "Release war room",
    createdByConversationId: 7,
    memberCount: 3,
    unreadCount: 0,
    createdAt: "2026-08-20T00:00:00.000Z",
    updatedAt: "2026-08-20T00:00:00.000Z",
    ...overrides,
  }
}

function renderWithIntl(ui: ReactElement) {
  return render(
    <NextIntlClientProvider locale="en" messages={enMessages}>
      {ui}
    </NextIntlClientProvider>
  )
}

describe("SessionRoomChip", () => {
  beforeEach(() => {
    h.openRoom.mockClear()
    h.ensure.mockClear()
    h.refresh.mockClear()
    h.state.rooms = []
    h.state.hydrated = true
  })

  it("shows the Room title and keeps the full id in the tooltip", () => {
    h.state.rooms = [room()]
    renderWithIntl(<SessionRoomChip roomId={ROOM_ID} />)

    const button = screen.getByRole("button", {
      name: "Open Room: Release war room",
    })
    expect(button).toHaveAttribute("title", `Release war room · ${ROOM_ID}`)
    expect(screen.getByText("Release war room")).toBeInTheDocument()
    // The bare UUID is tooltip-only once the title resolves.
    expect(screen.queryByText(ROOM_ID)).not.toBeInTheDocument()
  })

  it("opens the Room through the shared entry point when clicked", () => {
    const target = room()
    h.state.rooms = [target]
    renderWithIntl(<SessionRoomChip roomId={ROOM_ID} />)

    fireEvent.click(screen.getByRole("button"))
    expect(h.openRoom).toHaveBeenCalledTimes(1)
    expect(h.openRoom).toHaveBeenCalledWith(target)
  })

  it("falls back to a truncated id and stays inert for an unknown Room", () => {
    h.state.rooms = [room({ id: "rm_other", title: "Another room" })]
    renderWithIntl(<SessionRoomChip roomId={ROOM_ID} />)

    // No dead link: an unresolvable Room is not clickable at all.
    expect(screen.queryByRole("button")).toBeNull()
    expect(screen.getByText("rm_1f9c2b7a")).toBeInTheDocument()
    // The tooltip still carries the untruncated original id.
    expect(
      screen.getByTitle(`Room unavailable · ${ROOM_ID}`)
    ).toBeInTheDocument()
    expect(screen.getByText("Room")).toBeInTheDocument()
  })

  it("keeps the Room label when the envelope carried no id", () => {
    renderWithIntl(<SessionRoomChip roomId={null} />)

    expect(screen.queryByRole("button")).toBeNull()
    expect(screen.getByText("Room")).toBeInTheDocument()
  })

  it("renders the chip from a Room-channel Session-letter card", () => {
    h.state.rooms = [room()]
    renderWithIntl(
      <SessionMailCard
        direction="inbound"
        channel="room"
        roomId={ROOM_ID}
        eventId="evt-1"
        body="check the plan"
      />
    )

    expect(
      screen.getByRole("button", { name: "Open Room: Release war room" })
    ).toBeInTheDocument()
  })

  it("kicks the catalog load once for a whole transcript of Room cards", () => {
    h.state.hydrated = false
    renderWithIntl(
      <>
        <SessionRoomChip roomId={ROOM_ID} />
        <SessionRoomChip roomId={ROOM_ID} />
      </>
    )

    expect(h.ensure).toHaveBeenCalled()
    expect(h.refresh).toHaveBeenCalledTimes(1)
  })
})
