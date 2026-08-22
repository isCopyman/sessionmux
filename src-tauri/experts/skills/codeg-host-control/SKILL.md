---
name: codeg-host-control
description: Use when a managed Codeg Agent needs to create, inspect, rename or retune Codeg Sessions, organize Sessions into Collections or Workbenches, or claim/refine task-board cards through Codeg's progressive Host Control MCP. Room create/add-member and posting belong to the codeg-room skill. Private mail belongs to codeg-mailbox. Team playbooks belong to codeg-multi-agent.
---

# Codeg Host Control

Use Codeg's own MCP gateway for Session and organization management. This is
the formal Agent path; do not substitute shell commands, a CLI fallback,
vendor tool search, database access, or UI automation.

## Workflow

1. Call `codeg_help` with `action` when you know the likely action, or with a
   short `query` when you need to discover it.
2. Read the returned server-owned input schema and access level.
3. Call `codeg_use` with exactly the advertised `action` and `input` fields.
4. Treat `read`, `persisted`, `ui_requested`, and `rejected` as distinct stages.
   There is no `not_found` stage — a missing Session, Collection, or Workbench
   comes back as `rejected` with a `note` naming what was not found.
   A result with `replayed: true` is the response to the same live-host MCP
   call. After a Host restart, list current state before repeating a create.

Never send a caller, source, `from`, current/self Session id, companion token,
or request id. Codeg derives caller identity from the companion token and mints
the idempotency key outside model-controlled arguments.

## Available Session actions

- `session.list`: list persistent Sessions in the caller's current project
  scope other than the caller. Use only the filters advertised by
  `codeg_help`.
- `session.get`: read metadata for one persistent Session in the caller's
  current project scope. It does not expose another Session's transcript.
  Both actions return `message_count` and `updated_at`. To tell whether a
  Session is actually working — spending turns and tokens — compare
  `message_count` across two reads. `updated_at` moves on bookkeeping
  writes as well, so a recent timestamp with an unchanged `message_count`
  means the Session is idle. Observed 2026-08-20.
- `session.rename`: persist a manual title. Omit `session_id` to rename the
  token-derived current Session; include it only when the user clearly named a
  different target Session.
- `session.create`: create a real persistent Session in the caller's current
  cwd, optionally with a first Prompt and optional `collection_id`. Placement
  happens after the Session exists; a Collection miss keeps the Session.
  It stays in the background unless the user later opens it.
  For this-run identity use `title` plus `initial_prompt`. That prompt is a
  user message, not a harness system prompt. Do not ask Host Control for a
  system prompt field. When the user wants a team assembled, follow
  `codeg-multi-agent` for who to create and what the first prompt should say.
  The required `harness` is the **wire id**: `claude_code`, `codex`,
  `open_code`, `gemini`, `open_claw`, `cline`, `hermes`, `code_buddy`,
  `kimi_code`, `pi`, `grok`, `cursor`, `deepseek`, `qoder`, or
  `custom:<id>`. Codeg's other id namespace — the ACP launch-registry ids
  `claude-acp`, `codex-acp`, `opencode`, `openclaw-acp`, `codebuddy-code`,
  `kimi-code`, `pi-acp`, `grok-build`, `deepseek-acp`, `qoder-cli`, or a
  bare custom `<id>` — describes binary installs and is rejected here with
  `unknown agent type`. The two spell alike for only four agents
  (`gemini`, `cline`, `hermes`, `cursor`), so never infer one from the
  other or from a display name: read the `agent_type` of a live Session
  via `session.list` (or mailbox `list_sessions`), which reports this same
  wire vocabulary. Value lists verified against the code 2026-08-20.
  A Room id can never appear in `initial_prompt` — `room.create` needs the
  Session ids, so the Room is younger than its members. Create the
  Sessions, create the Room, then brief the team with a Room post.
- `session.cancel_turn`: cancel only the active Turn and keep the Session/runtime.
- `session.stop`: stop the managed runtime while preserving Session identity and
  native history for resume.
- `session.get_selectors`: read a Session's model and thinking-effort
  selectors — the Host-pinned values plus, while a runtime is live, the
  Harness-advertised current value and available choices.
- `session.set_selectors`: pin a Session's model and/or thinking effort (at
  least one is required). The pin applies at every later runtime (re)start and
  wins over the user's saved per-agent defaults; with a live runtime the switch
  is also requested immediately for subsequent Turns. Read
  `session.get_selectors` first and pick from the advertised choices — effort
  vocabularies differ per Harness.
- `session.archive` / `session.unarchive`: hide or restore a Session in the
  caller's current project scope. An archived Session refuses new mail
  (the sender is told) and Room `@` mentions to it come back
  `failed (target_archived)`; the Session, transcript, and runtime are kept.
  Archiving waives every reply others still owe it — final, restoring does
  not revive them — while its own unpaid replies stay frozen (unreminded)
  until it is restored. Omit `session_id` to archive or restore the
  token-derived current Session.

## Available organization actions

- `collection.list/create/rename/move`: manage the Collection tree in the
  caller's Path scope by stable IDs.
- `collection.add_session`: set a Session's one primary Collection. This may
  move it from its prior Collection but never changes cwd or runtime.
- `collection.remove_session`: remove only the named primary membership and
  leave the Session Unclassified.
- `workbench.list/create/rename`: manage the app-wide saved Workbench namespace.
  Workbenches may mix Paths; `list` returns only Session memberships visible in
  the caller's Path and flags hidden memberships. Create and rename do not mount,
  switch to, or focus a Workbench.
- `workbench.add_session/remove_session`: persist or remove a Session reference
  without deleting/stopping the Session or requesting focus.
- `workbench.place_session`: explicitly switch the connected workspace UI to a
  Workbench, focus the Session, and optionally place it in a right/down split.
  Use this UI-affecting action only when the user asked to open or arrange the
  Workbench. `ui_requested` is not proof that a disconnected View applied it.

## Available task actions

Task creation and execution choice are separate. `create_work_task` captures an
unassigned card; use `initial_status=backlog` for an uncommitted idea or omit it
for a ready Todo. Neither starts an Agent, selects a Harness, Profile, model,
Session, or worktree. Use `list_tasks` / `get_task` to inspect cards, then Host
Control for:

- `task.update`: refine the title and/or description of an unassigned card or
  the caller's own active card. Editing an active card updates the board record;
  it does not rewrite a Prompt already submitted to the Harness.
- `task.claim`: atomically assign an unassigned board card to the token-derived
  current persistent Session and queue its brief as that Session's next normal
  prompt, moving it to In Progress. Never pass a Session id. A competing claim
  is rejected rather than stealing the card.
- `task.assign`: the same atomic path, with an optional stable
  `target_session_id` for assigning another Session. Omit it to claim for self.

While working, use `task_progress` at meaningful milestones and
`task_complete` with `blocked` or a review-ready verdict. An Agent may move its
work to Blocked or Review; only the human accepts it as Done, cancels it, or
reopens it. The visible board has seven fixed, hideable columns and seven
fixed business states: `backlog`, `todo`, `in_progress`, `blocked`, `review`,
`done`, and `canceled`. A human may freely move a neutral/manual card between
these columns; moving a card never starts or wakes an Agent. Keep long-lived
plans, evidence, and deliverables in project files, and put only a concise
summary plus stable file paths on the task card or its progress timeline.

## Room actions live in `codeg-room`

`room.create`, `room.add_member`, and `room.list` are still Host Control
actions (`codeg_help` / `codeg_use`), but when to create a Room, how to
post, and how `@` works belong to the `codeg-room` skill. Private mail
belongs to `codeg-mailbox` (`send_message` without a Room id). Flexible
team playbooks belong to `codeg-multi-agent`.

Do not post through Host Control. Do not answer a Room mention with
`send_message`.

Workbench Pane layout, window mounts and focus remain device-local.
`workbench.place_session` exposes only the existing tab/right/down placement
primitive; arbitrary layout save/apply is still unavailable. A persisted
Workbench membership is not proof that a user saw it. Do not simulate layout or
focus through UI automation. Session import/resume/fork are also
unavailable unless `codeg_help` advertises them.

Before any write, discover the exact action schema and use stable numeric IDs
returned by `session.list`, `collection.list`, or `workbench.list`.
`codeg_help` returns schemas, not object IDs. Never use a display name as an
address. If an action is not
advertised, report it as unavailable instead of using another channel.
