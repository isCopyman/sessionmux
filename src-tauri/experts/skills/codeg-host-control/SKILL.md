---
name: codeg-host-control
description: Use when a managed Codeg Agent needs to inspect or rename Codeg Sessions through Codeg's progressive Host Control MCP.
---

# Codeg Host Control

Use Codeg's own MCP gateway for Codeg Session management. This is the formal
Agent path; do not substitute shell commands, a CLI fallback, vendor tool
search, database access, or UI automation.

## Workflow

1. Call `codeg_help` with `action` when you know the likely action, or with a
   short `query` when you need to discover it.
2. Read the returned server-owned input schema and access level.
3. Call `codeg_use` with exactly the advertised `action` and `input` fields.
4. Treat `read`, `persisted`, `not_found`, and `rejected` as distinct stages.
   A result with `replayed: true` is the response to the same idempotent MCP
   call and must not be repeated as a new user-visible change.

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

Session create/import/resume/fork/archive/open/focus and Collection/Workbench
actions are not in this first capability slice. If `codeg_help` does not
advertise an action, say it is currently unavailable; do not simulate it via
another channel.
