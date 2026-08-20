import { beforeEach, describe, expect, it, vi } from "vitest"

vi.mock("@/lib/workbench-session-tabs", () => ({
  locateConversationHome: vi.fn(),
}))

import { locateConversationHome } from "@/lib/workbench-session-tabs"

import { openOrFocusSession } from "./open-or-focus-session"

const locate = vi.mocked(locateConversationHome)

function session(id: number) {
  return {
    id,
    folder_id: 7,
    agent_type: "codex" as const,
    title: `S${id}`,
  }
}

describe("openOrFocusSession", () => {
  const switchWorkbench = vi.fn()
  const openTab = vi.fn()
  const openConversations = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
    switchWorkbench.mockResolvedValue(undefined)
  })

  it("opens on the current workbench when the session has no home", async () => {
    locate.mockResolvedValue(null)
    await expect(
      openOrFocusSession({
        conversation: session(4),
        currentWorkbenchId: 1,
        switchWorkbench,
        openTab,
        openConversations,
      })
    ).resolves.toBe("opened")
    expect(switchWorkbench).not.toHaveBeenCalled()
    expect(openConversations).toHaveBeenCalled()
    expect(openTab).toHaveBeenCalledWith(7, 4, "codex", true, "S4", undefined)
  })

  it("focuses in place when the home is the current workbench", async () => {
    locate.mockResolvedValue({
      conversation_id: 4,
      workbench_id: 1,
      workbench_name: "Main",
      workbench_position: 0,
    })
    await expect(
      openOrFocusSession({
        conversation: session(4),
        currentWorkbenchId: 1,
        switchWorkbench,
        openTab,
      })
    ).resolves.toBe("focused")
    expect(switchWorkbench).not.toHaveBeenCalled()
    expect(openTab).toHaveBeenCalled()
  })

  it("switches workbench then focuses when the home is elsewhere", async () => {
    locate.mockResolvedValue({
      conversation_id: 4,
      workbench_id: 3,
      workbench_name: "Other",
      workbench_position: 1,
    })
    await expect(
      openOrFocusSession({
        conversation: session(4),
        currentWorkbenchId: 1,
        switchWorkbench,
        openTab,
        openConversations,
      })
    ).resolves.toBe("switched")
    expect(switchWorkbench).toHaveBeenCalledWith(3)
    expect(openConversations).toHaveBeenCalled()
    expect(openTab).toHaveBeenCalledWith(7, 4, "codex", true, "S4")
  })
})
