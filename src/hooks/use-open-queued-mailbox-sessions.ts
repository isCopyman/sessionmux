"use client"

import { useEffect, useRef } from "react"

import { useTabActions, useTabStore } from "@/contexts/tab-context"
import { useAppWorkspaceStore } from "@/stores/app-workspace-store"
import { getCollaborationFeed } from "@/lib/api"
import { onTransportReconnect, subscribe } from "@/lib/platform"
import { formatConversationTitle } from "@/lib/conversation-title"
import type { CollaborationChanged, CollaborationFeed } from "@/lib/types"
import { COLLABORATION_CHANGED_EVENT } from "./use-collaboration-feed"

export function mailboxSessionNeedsWorkbench(feed: CollaborationFeed): boolean {
  return feed.inbound.some((delivery) => {
    const unreadQueued =
      delivery.invocationPolicy === "invoke_when_idle" &&
      (delivery.state === "queued" || delivery.state === "embedding") &&
      delivery.agentReceivedAt == null
    const awaitingReply =
      delivery.obligationState === "awaiting_reply" && !delivery.replyReceived
    return unreadQueued || awaitingReply
  })
}

/**
 * When mailbox attention needs a live Session that has no workbench tab,
 * open that Session so the existing connect path can attach to the
 * dispatcher-started runtime — the same surface a human send would activate.
 */
export function useOpenQueuedMailboxSessions() {
  const { openTab } = useTabActions()
  const openedIds = useRef(new Set<number>())

  useEffect(() => {
    let disposed = false
    let unsubscribe: (() => void) | undefined

    const openIfQueued = async (conversationIds: number[]) => {
      const conversations = useAppWorkspaceStore.getState().conversations
      const tabs = useTabStore.getState().rawTabs
      const openConversationIds = new Set(
        tabs
          .map((tab) => tab.conversationId)
          .filter((id): id is number => id != null)
      )
      for (const conversationId of conversationIds) {
        if (openedIds.current.has(conversationId)) continue
        if (openConversationIds.has(conversationId)) {
          openedIds.current.add(conversationId)
          continue
        }
        let feed
        try {
          feed = await getCollaborationFeed(conversationId)
        } catch {
          continue
        }
        if (disposed) return
        if (!mailboxSessionNeedsWorkbench(feed)) continue
        const conversation = conversations.find(
          (item) => item.id === conversationId
        )
        if (!conversation) continue
        openedIds.current.add(conversationId)
        openTab(
          conversation.folder_id,
          conversation.id,
          conversation.agent_type,
          true,
          formatConversationTitle(conversation.title) || undefined
        )
      }
    }

    void subscribe<CollaborationChanged>(
      COLLABORATION_CHANGED_EVENT,
      (change) => {
        if (!disposed && change.conversationIds.length > 0) {
          void openIfQueued(change.conversationIds)
        }
      }
    ).then((off) => {
      if (disposed) off()
      else unsubscribe = off
    })
    const offReconnect = onTransportReconnect(() => {
      openedIds.current.clear()
    })
    return () => {
      disposed = true
      unsubscribe?.()
      offReconnect?.()
    }
  }, [openTab])
}
