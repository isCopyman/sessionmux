"use client"

import { create } from "zustand"

export type SessionLetterRenderMode = "custom" | "mcp"

const STORAGE_KEY = "codeg.session-letter-render"

function readMode(): SessionLetterRenderMode {
  if (typeof window === "undefined") return "custom"
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)
    return raw === "mcp" ? "mcp" : "custom"
  } catch {
    return "custom"
  }
}

export const useSessionLetterRenderStore = create<{
  mode: SessionLetterRenderMode
  setMode: (mode: SessionLetterRenderMode) => void
}>((set) => ({
  mode: readMode(),
  setMode: (mode) => {
    try {
      window.localStorage.setItem(STORAGE_KEY, mode)
    } catch {
      // Display preference only.
    }
    set({ mode })
  },
}))
