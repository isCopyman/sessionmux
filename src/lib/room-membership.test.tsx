import type { ReactNode } from "react"
import { renderHook } from "@testing-library/react"
import { NextIntlClientProvider } from "next-intl"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"

import { useRoomMembership } from "./room-membership"
import enMessages from "@/i18n/messages/en.json"

const h = vi.hoisted(() => ({
  createCollaborationRoom: vi.fn(),
  addCollaborationRoomMembers: vi.fn(),
}))

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock("@/lib/api", () => ({
  createCollaborationRoom: h.createCollaborationRoom,
  addCollaborationRoomMembers: h.addCollaborationRoomMembers,
}))

function wrapper({ children }: { children: ReactNode }) {
  return (
    <NextIntlClientProvider locale="en" messages={enMessages}>
      {children}
    </NextIntlClientProvider>
  )
}

describe("useRoomMembership", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("createRoomWith toasts success and returns the room", async () => {
    const created = { id: "rm_1", title: "Alpha's room" }
    h.createCollaborationRoom.mockResolvedValue(created)
    const { result } = renderHook(() => useRoomMembership(), { wrapper })

    const returned = await result.current.createRoomWith({
      workbenchId: 1,
      title: "Alpha's room",
      memberConversationIds: [1],
      createdByConversationId: 1,
    })

    expect(returned).toEqual(created)
    expect(toast.success).toHaveBeenCalledWith("Created room Alpha's room")
  })

  it("createRoomWith toasts the failure and returns null", async () => {
    h.createCollaborationRoom.mockRejectedValue(new Error("nope"))
    const { result } = renderHook(() => useRoomMembership(), { wrapper })

    const returned = await result.current.createRoomWith({
      workbenchId: 1,
      title: "Alpha's room",
      memberConversationIds: [1],
      createdByConversationId: 1,
    })

    expect(returned).toBeNull()
    expect(toast.error).toHaveBeenCalledWith("Could not create the room: nope")
  })

  it("addMembersTo toasts success and returns the updated room", async () => {
    const updated = { id: "rm_1", title: "Plan" }
    h.addCollaborationRoomMembers.mockResolvedValue(updated)
    const { result } = renderHook(() => useRoomMembership(), { wrapper })

    const returned = await result.current.addMembersTo("rm_1", [2])

    expect(h.addCollaborationRoomMembers).toHaveBeenCalledWith({
      roomId: "rm_1",
      conversationIds: [2],
    })
    expect(returned).toEqual(updated)
    expect(toast.success).toHaveBeenCalledWith("Session added to the room")
  })
})
