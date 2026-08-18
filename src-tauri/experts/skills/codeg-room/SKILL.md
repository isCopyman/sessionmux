---
name: codeg-room
description: Use when a managed Codeg Agent needs to read or post in a shared Room (list_rooms, read_room, post_room). Do not use for private mailbox letters, creating Sessions, or talking to AgentBus.
---

# Codeg Room

A **Room** is a shared ledger. Every member can read the same posts.
Empty mentions are record-only: nobody is woken. Structured mentions
create a delivery so that Session looks at the Room.

This is not private mail. Do not use `send_message` / `list_inbox` /
`read_message` for Room work.

## Tools

- `list_rooms`: Rooms this Session already belongs to. Use the returned
  `room_id`.
- `read_room`: members plus timeline bodies. Call this after a Room
  mention envelope.
- `post_room`: write to the Room.
  - Omit `mention_session_ids` (and `mention_all=false`) to record only.
  - Pass `mention_session_ids` or `mention_all=true` to tap a shoulder.
  - Reply or supplement with the same `room_id` and `reply_to_event_id`.

Lifecycle (create, add a member) is Host Control via `codeg_help` /
`codeg_use`:

- `room.create` — you become owner; pass at least one other Session id
- `room.add_member` — add a Session you already share a Room with
- `room.list` — Workbench-wide list (prefer `list_rooms` for your own)

## Workflow

1. If you do not have a `room_id`, call `list_rooms`.
2. `read_room` before answering a mention so you see the public thread.
3. Reply with `post_room`. Default back to the Room, not to private mail.
4. `@` only the Sessions that must act. Do not `@all` unless asked.

## Threads

Same rule as mail: omitting `reply_to_event_id` starts a new root.
A follow-up on the same Room thread must set it.

## Hard rules

- You must already be a member to `read_room` or `post_room`.
- Mentions must already be members.
- Never answer a Room mention with `send_message`.
- Never copy a Room post into a private letter unless asked.
- If these tools are missing, say Room collaboration is unavailable.
