import { describe, expect, it } from "vitest"

import { denormalizeSnapshot } from "@/lib/snapshot-denormalize"
import type { LiveSessionSnapshot, ToolCallState } from "@/lib/types"

function baseSnapshot(
  overrides: Partial<LiveSessionSnapshot> = {}
): LiveSessionSnapshot {
  return {
    connection_id: "conn-1",
    conversation_id: null,
    folder_id: null,
    status: "connected",
    external_id: null,
    live_message: null,
    active_tool_calls: [],
    pending_permission: null,
    modes: null,
    current_mode: null,
    config_options: null,
    prompt_capabilities: null,
    usage: null,
    fork_supported: false,
    available_commands: [],
    selectors_ready: false,
    event_seq: 0,
    ...overrides,
  }
}

describe("denormalizeSnapshot — subagent attribution on live blocks", () => {
  it("forwards parent_tool_use_id onto text/thinking, absent field stays undefined", () => {
    const patch = denormalizeSnapshot(
      baseSnapshot({
        live_message: {
          id: "lm-1",
          role: "assistant",
          started_at: "2026-07-28T00:00:00Z",
          content: [
            { kind: "text", text: "main" },
            { kind: "text", text: "sub", parent_tool_use_id: "toolu_p" },
            {
              kind: "thinking",
              text: "sub think",
              parent_tool_use_id: "toolu_p",
            },
          ],
        },
      })
    )
    const content = patch.liveMessage?.content ?? []
    expect(content).toHaveLength(3)
    expect(content[0]).toMatchObject({ type: "text", text: "main" })
    expect(
      content[0]?.type === "text" ? content[0].parentToolUseId : "SET"
    ).toBeUndefined()
    expect(content[1]).toMatchObject({
      type: "text",
      text: "sub",
      parentToolUseId: "toolu_p",
    })
    expect(content[2]).toMatchObject({
      type: "thinking",
      text: "sub think",
      parentToolUseId: "toolu_p",
    })
  })
})

describe("denormalizeSnapshot — config staleness", () => {
  it("carries config_stale / config_stale_kind into the patch", () => {
    const patch = denormalizeSnapshot(
      baseSnapshot({ config_stale: true, config_stale_kind: "model_provider" })
    )
    expect(patch.configStale).toBe(true)
    expect(patch.configStaleKind).toBe("model_provider")
  })

  it("defaults to not-stale when the fields are absent (older server payload)", () => {
    const snap = baseSnapshot()
    delete (snap as { config_stale?: unknown }).config_stale
    delete (snap as { config_stale_kind?: unknown }).config_stale_kind
    const patch = denormalizeSnapshot(snap)
    expect(patch.configStale).toBe(false)
    expect(patch.configStaleKind).toBeNull()
  })
})

describe("denormalizeSnapshot — last_error", () => {
  it("carries last_error.message into the patch", () => {
    const patch = denormalizeSnapshot(
      baseSnapshot({
        last_error: {
          message: " ACP protocol error: Forbidden ",
          code: "forbidden",
        },
      })
    )
    expect(patch.lastError).toBe("ACP protocol error: Forbidden")
    expect(patch.lastErrorCode).toBe("forbidden")
    expect(patch.status).toBe("connected")
  })

  it("defaults lastError to null when the field is absent", () => {
    const snap = baseSnapshot()
    delete (snap as { last_error?: unknown }).last_error
    const patch = denormalizeSnapshot(snap)
    expect(patch.lastError).toBeNull()
    expect(patch.lastErrorCode).toBeNull()
    expect(patch.status).toBe("connected")
  })
})

describe("denormalizeSnapshot — tagged tool output", () => {
  function hydrateTool(output: ToolCallState["output"]) {
    return denormalizeSnapshot(
      baseSnapshot({
        live_message: {
          id: "lm-tool",
          role: "assistant",
          started_at: "2026-08-23T00:00:00Z",
          content: [{ kind: "tool_call_ref", tool_call_id: "tc-1" }],
        },
        active_tool_calls: [
          {
            id: "tc-1",
            kind: "execute",
            label: "Tool",
            status: "completed",
            input: null,
            output,
            content: null,
            locations: null,
            meta: null,
          },
        ],
      })
    ).liveMessage?.content[0]
  }

  it("hydrates JSON output as its payload rather than the tagged wrapper", () => {
    const block = hydrateTool({ kind: "json", value: { answer: 42 } })
    expect(block).toMatchObject({
      type: "tool_call",
      info: { raw_output_chunks: ['{"answer":42}'] },
    })
  })

  it("preserves empty text and error output", () => {
    expect(hydrateTool({ kind: "text", content: "" })).toMatchObject({
      info: { raw_output_chunks: [""] },
    })
    expect(hydrateTool({ kind: "error", message: "boom" })).toMatchObject({
      info: { raw_output_chunks: ['{"error":"boom"}'] },
    })
  })
})
