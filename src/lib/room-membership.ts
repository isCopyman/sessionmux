"use client"

import { useCallback } from "react"
import { useTranslations } from "next-intl"
import { toast } from "sonner"

import { addCollaborationRoomMembers, createCollaborationRoom } from "@/lib/api"
import { toErrorMessage } from "@/lib/app-error"
import type {
  CollaborationRoomDetail,
  CreateCollaborationRoomInput,
} from "@/lib/types"

/**
 * Shared Room membership writes: call the existing API, toast success/failure
 * with the keys the three call sites already used, and return the payload so
 * the caller can open the room / refresh local state. Does not throw on API
 * failure (null) so callers do not double-toast.
 */
export function useRoomMembership() {
  const t = useTranslations("Room")
  const tManage = useTranslations("Folder.sidebar.manageConversations")

  const createRoomWith = useCallback(
    async (
      input: CreateCollaborationRoomInput
    ): Promise<CollaborationRoomDetail | null> => {
      try {
        const created = await createCollaborationRoom(input)
        toast.success(tManage("toastRoomCreated", { title: created.title }))
        return created
      } catch (error) {
        toast.error(t("createFailed", { message: toErrorMessage(error) }))
        return null
      }
    },
    [t, tManage]
  )

  const addMembersTo = useCallback(
    async (
      roomId: string,
      conversationIds: number[]
    ): Promise<CollaborationRoomDetail | null> => {
      try {
        const updated = await addCollaborationRoomMembers({
          roomId,
          conversationIds,
        })
        toast.success(t("added"))
        return updated
      } catch (error) {
        toast.error(toErrorMessage(error))
        return null
      }
    },
    [t]
  )

  return { createRoomWith, addMembersTo }
}
