import { beforeEach, describe, expect, it, vi } from "vitest"

import {
  acpConnect,
  conversationSetLaunchPreferences,
  createConversation,
  workTaskAssignSession,
} from "@/lib/api"
import { conversationSetClaudeProfile } from "@/lib/api"
import type { WorkTask, WorkTaskConfig } from "@/lib/types"
import { createTaskSessionAndAssign } from "./task-session-launch"

vi.mock("@/lib/api", () => ({
  acpConnect: vi.fn(),
  createConversation: vi.fn(),
  conversationSetClaudeProfile: vi.fn(),
  conversationSetLaunchPreferences: vi.fn(),
  workTaskAssignSession: vi.fn(),
}))

const task = {
  id: 7,
  folder_id: 3,
  title: "Review the citations",
} as WorkTask

const config = {
  prompt_blocks: [],
  display_text: "",
  agent_type: "claude_code",
  mode_id: "plan",
  config_values: {
    __codeg_profile__: "cpa",
    model: "fable",
  },
  label_snapshot: null,
} satisfies WorkTaskConfig

describe("createTaskSessionAndAssign", () => {
  beforeEach(() => {
    vi.mocked(createConversation).mockReset().mockResolvedValue(42)
    vi.mocked(conversationSetClaudeProfile).mockReset().mockResolvedValue({
      conversationId: 42,
      profileId: "cpa",
      affectedRunningSessions: 0,
    })
    vi.mocked(acpConnect).mockReset().mockResolvedValue("connection-42")
    vi.mocked(conversationSetLaunchPreferences).mockReset().mockResolvedValue()
    vi.mocked(workTaskAssignSession)
      .mockReset()
      .mockResolvedValue({} as never)
  })

  it("allocates identity and profile before ACP, then assigns the task", async () => {
    const order: string[] = []
    vi.mocked(createConversation).mockImplementation(async () => {
      order.push("identity")
      return 42
    })
    vi.mocked(conversationSetClaudeProfile).mockImplementation(async () => {
      order.push("profile")
      return {
        conversationId: 42,
        profileId: "cpa",
        affectedRunningSessions: 0,
      }
    })
    vi.mocked(acpConnect).mockImplementation(async () => {
      order.push("acp")
      return "connection-42"
    })
    vi.mocked(conversationSetLaunchPreferences).mockImplementation(async () => {
      order.push("selectors")
    })
    vi.mocked(workTaskAssignSession).mockImplementation(async () => {
      order.push("assign")
      return {} as never
    })

    await expect(
      createTaskSessionAndAssign({
        task,
        folderPath: "/repo",
        config,
      })
    ).resolves.toBe(42)

    expect(order).toEqual(["identity", "profile", "selectors", "acp", "assign"])
    expect(conversationSetLaunchPreferences).toHaveBeenCalledWith(42, "plan", {
      model: "fable",
    })
    expect(acpConnect).toHaveBeenCalledWith(
      "claude_code",
      "/repo",
      undefined,
      "plan",
      { model: "fable" },
      42,
      true
    )
  })

  it("does not assign when the Harness launch fails", async () => {
    vi.mocked(acpConnect).mockRejectedValueOnce(new Error("launch failed"))

    await expect(
      createTaskSessionAndAssign({
        task,
        folderPath: "/repo",
        config,
      })
    ).rejects.toThrow("launch failed")
    expect(workTaskAssignSession).not.toHaveBeenCalled()
  })
})
