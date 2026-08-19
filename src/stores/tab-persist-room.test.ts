import { describe, expect, it } from "vitest"

import {
  buildPersistItems,
  isDraftTab,
  makeRoomTabId,
  resetTabStore,
  useTabStore,
} from "./tab-store"
import type { TabItemInternal } from "./tab-store"
import {
  resetAppWorkspaceStore,
  useAppWorkspaceStore,
} from "./app-workspace-store"
import type { FolderDetail } from "@/lib/types"

function sessionTab(id: number): TabItemInternal {
  return {
    id: `conv-1-codex-${id}`,
    kind: "conversation",
    folderId: 1,
    conversationId: id,
    agentType: "codex",
    title: `Session ${id}`,
    isPinned: true,
  }
}

function draftTab(): TabItemInternal {
  return {
    id: "new-draft",
    kind: "conversation",
    folderId: 1,
    conversationId: null,
    agentType: "gemini",
    title: "New",
    isPinned: true,
  }
}

function roomTab(roomId: string): TabItemInternal {
  return {
    id: makeRoomTabId(roomId),
    kind: "room",
    roomId,
    folderId: 1,
    conversationId: null,
    agentType: "claude_code",
    title: "Plan",
    isPinned: true,
  }
}

describe("Room tab persistence payload", () => {
  it("persists Room tabs with Session tabs and drops drafts", () => {
    const room = roomTab("rm_plan")
    const items = buildPersistItems([sessionTab(11), room, draftTab()], room.id)
    expect(items).toEqual([
      {
        id: 0,
        folder_id: 1,
        conversation_id: 11,
        room_id: null,
        agent_type: "codex",
        position: 0,
        is_active: false,
        is_pinned: true,
      },
      {
        id: 0,
        folder_id: 1,
        conversation_id: null,
        room_id: "rm_plan",
        agent_type: "claude_code",
        position: 1,
        is_active: true,
        is_pinned: true,
      },
    ])
  })

  it("does not treat a Room tab as a composer draft", () => {
    expect(isDraftTab(roomTab("rm_plan"))).toBe(false)
    expect(isDraftTab(draftTab())).toBe(true)
    expect(isDraftTab(sessionTab(11))).toBe(false)
  })
})

describe("New conversation with a Room tab in the group", () => {
  it("creates a fresh draft instead of focusing the Room tab", () => {
    resetAppWorkspaceStore()
    const folder = {
      id: 1,
      path: "/w",
      name: "w",
      kind: "git",
    } as unknown as FolderDetail
    useAppWorkspaceStore.setState({
      folders: [folder],
      allFolders: [folder],
      foldersHydrated: true,
    })
    localStorage.clear()
    resetTabStore()
    const room = roomTab("rm_plan")
    useTabStore.setState({ rawTabs: [room], activeTabId: room.id })

    useTabStore.getState().openNewConversationTab(1, "/w")

    const st = useTabStore.getState()
    expect(st.rawTabs).toHaveLength(2)
    const draft = st.rawTabs.find((t) => t.id !== room.id)
    expect(draft?.kind).toBe("conversation")
    expect(draft?.conversationId).toBeNull()
    expect(st.activeTabId).toBe(draft?.id)
  })
})
