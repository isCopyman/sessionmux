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

## How Codeg marks a Room @

When someone `@`s you in a Room, Codeg injects a
`CODEG_SESSION_MESSAGE_V1` envelope (same wrapper as mailbox, different
channel):

- JSON: `"channel":"room"`, `"kind":"room_mention"`, plus `roomId`
- Host line: `channel=room`
- Queue label: `Codeg room: …`

That is **not** a mailbox letter. `list_inbox` will not show it.
Reply with `post_room` and `reply_to_event_id`. Never `send_message`
unless the operator asked for a private letter.

A `codeg://session/<id>` link is only an address. It does not pick
Room vs mailbox. The verb is the tool: `post_room` here,
`send_message` for private mail, `get_session_info` when the current
human turn only pointed at a Session.

Free-text `@alice` never wakes anyone. Use `mention_session_ids`,
`mention_all`, or `mention_human`.

## Tools

- `list_rooms`: Rooms this Session already belongs to. Includes
  `unread_count` (channel posts since you last read) and
  `mention_unread_count` (`@` deliveries you have not consumed).
- `read_room`: members plus timeline bodies.
  - Default: newest window. Surrounding context for a mention.
  - `unread=true`: every post since your last-read cursor, including
    posts that did not `@` you. Advances the cursor.
  - `before_event_id`: page older.
  - Opening consumes `@` deliveries in the returned window (marks them
    read). It does not clear a reply obligation.
  A Room mention envelope already includes that post's body.
- `post_room`: write to the Room.
  - Omit `mention_session_ids` (and `mention_all=false`) to record only.
  - Pass `mention_session_ids` or `mention_all=true` to tap a Session.
    An archived Session is still named on the timeline; it is not woken.
  - Pass `mention_human=true` (or `codeg://human`) to tap the operator.
    That does not wake a Session.
  - File / path text in the body is context, not a Delivery.

Lifecycle (create, add a member) is Host Control via `codeg_help` /
`codeg_use`:

- `room.create` — you become owner; pass at least one other Session id
- `room.add_member` — add a Session you already share a Room with
- `room.list` — Workbench-wide list (prefer `list_rooms` for your own)

## Workflow

1. If you do not have a `room_id`, call `list_rooms`.
2. The mention envelope already has that post. Call `read_room` when you
   need surrounding posts, a truncated remainder, or `unread=true` to
   catch up on posts you missed (including posts that did not `@` you).
3. Reply with `post_room` and `reply_to_event_id`. Default back to the
   Room, not to private mail. Host will nag if a mention asked for a
   reply and five minutes pass with no linked `post_room`.
4. `@` only the Sessions that must act. Do not `@all` unless asked.

## Threads

Same rule as mail: omitting `reply_to_event_id` starts a new root.
A follow-up on the same Room thread must set it.

## Hard rules

- You must already be a member to `read_room` or `post_room`.
- Mentions must already be members.
- Never answer a Room mention (`channel=room`) with `send_message`.
- Never copy a Room post into a private letter unless asked.
- A `codeg://session/<id>` badge in the current user turn is context,
  not an automatic Room mention. Use `mention_session_ids` (or
  `mention_all`) when you intend to wake someone in this Room.
- If these tools are missing, say Room collaboration is unavailable.
