"use client"

import { create } from "zustand"

export const useSessionLetterUiStore = create<{
  mcpPreviewKeys: Record<string, true>
  pendingFocus: { conversationId: number; eventId: string } | null
  focusedEventId: string | null
  isMcpPreview: (key: string) => boolean
  togglePreview: (key: string) => void
  requestFocus: (conversationId: number, eventId: string) => void
  consumeFocus: (conversationId: number) => { eventId: string } | null
  markFocused: (eventId: string) => void
}>((set, get) => ({
  mcpPreviewKeys: {},
  pendingFocus: null,
  focusedEventId: null,
  isMcpPreview: (key) => get().mcpPreviewKeys[key] === true,
  togglePreview: (key) =>
    set((state) => {
      const next = { ...state.mcpPreviewKeys }
      if (next[key]) delete next[key]
      else next[key] = true
      return { mcpPreviewKeys: next }
    }),
  requestFocus: (conversationId, eventId) =>
    set({ pendingFocus: { conversationId, eventId } }),
  consumeFocus: (conversationId) => {
    const pending = get().pendingFocus
    if (!pending || pending.conversationId !== conversationId) return null
    set({ pendingFocus: null })
    return { eventId: pending.eventId }
  },
  markFocused: (eventId) => {
    set({ focusedEventId: eventId })
    window.setTimeout(() => {
      if (get().focusedEventId === eventId) set({ focusedEventId: null })
    }, 2400)
  },
}))
