---
name: codeg-host-control
description: Use when a managed Codeg Agent needs to create, inspect or rename Codeg Sessions, organize Sessions into Collections, manage saved Workbenches, or create/list shared Rooms through Codeg's progressive Host Control MCP. Posting in a Room uses the post_room tool from the codeg-room skill, not Host Control.
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
4. Treat `read`, `persisted`, `ui_requested`, `not_found`, and `rejected` as distinct stages.
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
  cwd, optionally with a first Prompt and optional `collection_id`. Placement
  happens after the Session exists; a Collection miss keeps the Session.
  It stays in the background unless the user later opens it.
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
- `workbench.place_session`: explicitly switch the connected workspace UI to a
  Workbench, focus the Session, and optionally place it in a right/down split.
  Use this UI-affecting action only when the user asked to open or arrange the
  Workbench. `ui_requested` is not proof that a disconnected View applied it.

## Available Room actions

- `room.list`: list Rooms on a Workbench (defaults to Workbench 1). Prefer
  `list_rooms` when you only need Rooms you already belong to.
- `room.create`: create a shared Room. You become owner; pass at least one
  other Session id.
- `room.add_member`: add an existing Session to a Room you already belong to.

Do not post through Host Control. Read and write the ledger with
`read_room` / `post_room` (see `codeg-room`). Private mail stays on
`send_message` without a Room id.

Room posts are never private mail. Do not answer a Room mention with
`send_message`.

Workbench Pane layout, window mounts and focus remain device-local.
`workbench.place_session` exposes only the existing tab/right/down placement
primitive; arbitrary layout save/apply is still unavailable. A persisted
Workbench membership is not proof that a user saw it. Do not simulate layout or
focus through UI automation. Session import/resume/fork/archive are also
unavailable unless `codeg_help` advertises them.

Before any write, discover the exact action schema and use stable numeric IDs
returned by `session.list`, `collection.list`, or `workbench.list`.
`codeg_help` returns schemas, not object IDs. Never use a display name as an
address. If an action is not
advertised, report it as unavailable instead of using another channel.
