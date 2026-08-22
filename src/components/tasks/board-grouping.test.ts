import { describe, expect, it } from "vitest"
import type { WorkTask, WorkTaskStatus } from "@/lib/types"
import { groupingShowsHeaders, segmentTasksForGrouping } from "./board-grouping"

function task(
  id: number,
  status: WorkTaskStatus,
  extra?: Partial<WorkTask>
): WorkTask {
  return {
    id,
    folder_id: 1,
    title: `t${id}`,
    config: null,
    status,
    task_status: extra?.task_status ?? "todo",
    execution_mode: extra?.execution_mode ?? "engine",
    failure_reason: null,
    last_error: null,
    run_seq: 0,
    sort_order: id,
    worktree_folder_id: null,
    conversation_id: null,
    connection_id: null,
    base_branch: null,
    base_sha: null,
    work_branch: null,
    cleanup_state: null,
    verdict: null,
    result_summary: null,
    files_changed: null,
    additions: null,
    deletions: null,
    merge_commit: null,
    preflight: null,
    archived_at: null,
    scheduled_at: null,
    created_at: "2026-08-01T00:00:00Z",
    updated_at: "2026-08-01T00:00:00Z",
    started_at: null,
    settled_at: null,
    finished_at: null,
    ...extra,
  }
}

const names = new Map<number, string>([
  [1, "alpha"],
  [2, "bravo"],
])

function opts(folderFilter: number | null = null) {
  return {
    folderNames: names,
    sessionNames: new Map<number, string>([
      [21, "Research GPT"],
      [22, "Writing Claude"],
    ]),
    folderFilter,
    agentLabel: (agentType: string) =>
      agentType === "claude_code" ? "Claude Code" : agentType,
  }
}

describe("groupingShowsHeaders", () => {
  it("hides headers when grouping is off", () => {
    expect(groupingShowsHeaders("none", null)).toBe(false)
    expect(groupingShowsHeaders("none", 1)).toBe(false)
  })

  it("hides project headers once a single folder is already the filter", () => {
    expect(groupingShowsHeaders("folder", 1)).toBe(false)
    expect(groupingShowsHeaders("folder", null)).toBe(true)
  })

  it("always shows agent headers, even with a folder filter", () => {
    expect(groupingShowsHeaders("agent", null)).toBe(true)
    expect(groupingShowsHeaders("agent", 1)).toBe(true)
  })

  it("always shows Session headers", () => {
    expect(groupingShowsHeaders("session", null)).toBe(true)
    expect(groupingShowsHeaders("session", 1)).toBe(true)
  })
})

describe("segmentTasksForGrouping", () => {
  it("returns one unlabeled segment in input order when grouping is none", () => {
    const tasks = [task(2, "todo"), task(1, "todo")]
    expect(segmentTasksForGrouping(tasks, "none", opts())).toEqual([
      { key: "all", label: null, ungrouped: false, tasks },
    ])
  })

  it("groups by project, counts each segment, keeps input order inside", () => {
    const tasks = [
      task(1, "todo", { folder_id: 2 }),
      task(2, "todo", { folder_id: 1 }),
      task(3, "todo", { folder_id: 2 }),
      task(4, "todo", { folder_id: 1 }),
    ]
    const segs = segmentTasksForGrouping(tasks, "folder", opts())
    expect(segs.map((s) => [s.label, s.tasks.map((t) => t.id)])).toEqual([
      ["alpha", [2, 4]],
      ["bravo", [1, 3]],
    ])
  })

  it("collapses project grouping to one unlabeled segment when a folder is filtered", () => {
    const tasks = [
      task(1, "todo", { folder_id: 1 }),
      task(2, "todo", { folder_id: 1 }),
    ]
    expect(segmentTasksForGrouping(tasks, "folder", opts(1))).toEqual([
      { key: "all", label: null, ungrouped: false, tasks },
    ])
  })

  it("puts tasks whose folder is unknown in the ungrouped bucket", () => {
    const tasks = [
      task(1, "todo", { folder_id: 1 }),
      task(2, "todo", { folder_id: 99 }),
    ]
    const segs = segmentTasksForGrouping(tasks, "folder", opts())
    expect(
      segs.map((s) => [s.label, s.ungrouped, s.tasks.map((t) => t.id)])
    ).toEqual([
      ["alpha", false, [1]],
      [null, true, [2]],
    ])
  })

  it("groups by agent and parks tasks without one last as ungrouped", () => {
    const tasks = [
      task(1, "running", { agent_type: "codex" }),
      task(2, "running"),
      task(3, "running", { agent_type: "claude_code" }),
      task(4, "running", { agent_type: "codex" }),
      task(5, "running", {
        config: {
          prompt_blocks: [],
          display_text: "",
          agent_type: "claude_code",
          config_values: {},
        },
      }),
    ]
    const segs = segmentTasksForGrouping(tasks, "agent", opts())
    expect(
      segs.map((s) => [s.label, s.ungrouped, s.tasks.map((t) => t.id)])
    ).toEqual([
      ["Claude Code", false, [3, 5]],
      ["codex", false, [1, 4]],
      [null, true, [2]],
    ])
  })

  it("groups by concrete owner Session rather than Harness", () => {
    const tasks = [
      task(1, "running", { conversation_id: 22, agent_type: "claude_code" }),
      task(2, "running", { conversation_id: 21, agent_type: "claude_code" }),
      task(3, "todo", { conversation_id: null }),
      task(4, "running", { conversation_id: 21, agent_type: "codex" }),
    ]
    const segs = segmentTasksForGrouping(tasks, "session", opts())
    expect(
      segs.map((s) => [s.label, s.ungrouped, s.tasks.map((t) => t.id)])
    ).toEqual([
      ["Research GPT", false, [2, 4]],
      ["Writing Claude", false, [1]],
      [null, true, [3]],
    ])
  })
})
