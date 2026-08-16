import { afterEach, describe, expect, it } from "vitest"
import type {
  DbConversationDetail,
  DbConversationSummary,
  MessageTurn,
} from "@/lib/types"
import {
  resetConversationRuntimeStore,
  useConversationRuntimeStore,
  type ConversationRuntimeSession,
} from "@/stores/conversation-runtime-store"

const C1 = 41
const C2 = 42
const S1 = "native-S1"
const S2 = "native-S2"

function summary(
  id: number,
  externalId: string,
  title: string
): DbConversationSummary {
  return {
    id,
    folder_id: 7,
    title,
    title_locked: true,
    agent_type: "claude_code",
    status: "pending_review",
    kind: "regular",
    model: "test-model",
    git_branch: "main",
    external_id: externalId,
    message_count: 2,
    child_count: 0,
    created_at: "2026-08-16T00:00:00.000Z",
    updated_at: "2026-08-16T00:00:00.000Z",
    archived_at: null,
    pinned_at: null,
    parent_id: null,
    parent_tool_use_id: null,
    delegation_call_id: null,
    origin_cwd: null,
  }
}

function turn(
  id: string,
  role: "user" | "assistant",
  text: string
): MessageTurn {
  return {
    id,
    role,
    blocks: [{ type: "text", text }],
    timestamp: "2026-08-16T00:00:00.000Z",
  }
}

function session(): ConversationRuntimeSession {
  const detail: DbConversationDetail = {
    summary: summary(C1, S1, "Original"),
    turns: [turn("u1", "user", "question"), turn("a1", "assistant", "answer")],
    session_stats: null,
    transcript_watermark: 2,
    in_flight_user_turn_id: null,
  }
  return {
    conversationId: C1,
    externalId: S1,
    dbConversationId: C1,
    detail,
    detailLoading: false,
    detailError: null,
    acpLoadError: null,
    localTurns: [],
    backgroundTurns: [],
    pendingBackgroundSettlements: [
      {
        toolUseId: "tool-C1",
        taskId: "task-C1",
        status: "completed",
        summary: "belongs to C1",
        result: null,
      },
    ],
    optimisticTurns: [turn("optimistic", "user", "must not move")],
    liveMessage: {
      id: "live",
      role: "assistant",
      content: [],
      startedAt: 1,
    },
    syncState: "awaiting_persist",
    activeTurnToken: "active-C1",
    lastTurnOwned: true,
    liveOwnsActiveTurn: true,
    delegationKickoffText: null,
    sessionStats: null,
    historyAssistantBaseline: 1,
    batchBoundaryIndex: 2,
    batchBoundaryPrefixHash: "C1-boundary",
    loadingOlderTurns: true,
    olderTurnsPrependEpoch: 3,
    pendingCleanup: true,
  }
}

afterEach(() => resetConversationRuntimeStore())

describe("immutable conversation fork runtime handoff", () => {
  it("retains C1/S1 and creates a clean C2/S2 view cache", () => {
    const c1 = session()
    useConversationRuntimeStore.setState({
      byConversationId: new Map([[C1, c1]]),
      conversationIdByExternalId: new Map([[S1, C1]]),
    })

    const officialC2 = summary(C2, S2, "[Fork] Original")
    useConversationRuntimeStore
      .getState()
      .actions.forkConversation(C1, C2, S2, officialC2)

    const state = useConversationRuntimeStore.getState()
    const retained = state.byConversationId.get(C1)
    const forked = state.byConversationId.get(C2)
    expect(retained).toBe(c1)
    expect(retained?.externalId).toBe(S1)
    expect(retained?.optimisticTurns).toHaveLength(1)
    expect(retained?.liveMessage?.id).toBe("live")
    expect(retained?.pendingBackgroundSettlements).toHaveLength(1)

    expect(forked?.conversationId).toBe(C2)
    expect(forked?.dbConversationId).toBe(C2)
    expect(forked?.externalId).toBe(S2)
    expect(forked?.detail?.summary).toEqual(officialC2)
    expect(forked?.detail?.turns).toEqual(c1.detail?.turns)
    expect(forked?.optimisticTurns).toEqual([])
    expect(forked?.liveMessage).toBeNull()
    expect(forked?.syncState).toBe("idle")
    expect(forked?.activeTurnToken).toBeNull()
    expect(forked?.pendingBackgroundSettlements).toEqual([])
    expect(forked?.lastTurnOwned).toBe(false)
    expect(forked?.liveOwnsActiveTurn).toBe(false)
    expect(forked?.delegationKickoffText).toBeNull()
    expect(forked?.historyAssistantBaseline).toBeNull()
    expect(forked?.batchBoundaryIndex).toBeNull()
    expect(forked?.batchBoundaryPrefixHash).toBeNull()
    expect(forked?.loadingOlderTurns).toBe(false)
    expect(forked?.pendingCleanup).toBe(false)
    expect(state.conversationIdByExternalId.get(S1)).toBe(C1)
    expect(state.conversationIdByExternalId.get(S2)).toBe(C2)
  })

  it("reconciles the event-created C2 cache with the later API summary", () => {
    const c1 = session()
    useConversationRuntimeStore.setState({
      byConversationId: new Map([[C1, c1]]),
      conversationIdByExternalId: new Map([[S1, C1]]),
    })
    const actions = useConversationRuntimeStore.getState().actions

    actions.forkConversation(C1, C2, S2)
    expect(
      useConversationRuntimeStore.getState().byConversationId.get(C2)?.detail
        ?.summary.title
    ).toBe("[Fork] Original")

    const officialC2 = {
      ...summary(C2, S2, "[Fork] Renamed before response"),
      updated_at: "2026-08-16T00:00:01.000Z",
    }
    actions.forkConversation(C1, C2, S2, officialC2)
    expect(
      useConversationRuntimeStore.getState().byConversationId.get(C2)?.detail
        ?.summary
    ).toEqual(officialC2)
    expect(
      useConversationRuntimeStore.getState().byConversationId.get(C1)?.detail
        ?.summary.title
    ).toBe("Original")
  })
})
