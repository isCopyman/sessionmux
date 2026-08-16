---
name: codeg-host-control
description: Use when a managed Codeg Agent needs to inspect or rename Codeg Sessions, organize Sessions into Collections, or manage saved Workbench membership through Codeg's progressive Host Control MCP.
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
4. Treat `read`, `persisted`, `not_found`, and `rejected` as distinct stages.
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
- `session.rename`: persist a manual title. Omit `session_id` to rename the
  token-derived current Session; include it only when the user clearly named a
  different target Session.
- `session.create`: create a real persistent Session in the caller's current
  cwd, optionally with a first Prompt. It stays in the background unless the
  user later opens it.
- `session.cancel_turn`: cancel only the active Turn and keep the Session/runtime.
- `session.stop`: stop the managed runtime while preserving Session identity and
  native history for resume.

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

Workbench Pane layout, window mounts and focus remain device-local and are not
in this capability slice. A persisted Workbench membership is not proof that a
user saw it. Do not claim a layout was applied and do not simulate layout/open/
focus through UI automation. Session import/resume/fork/archive/open/focus are
also unavailable unless `codeg_help` advertises them.

Before any write, discover the exact action schema and use stable numeric IDs
returned by `session.list`, `collection.list`, or `workbench.list`.
`codeg_help` returns schemas, not object IDs. Never use a display name as an
address. If an action is not
advertised, report it as unavailable instead of using another channel.
