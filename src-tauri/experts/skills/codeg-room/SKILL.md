---
name: codeg-room
description: Use when a managed Codeg Agent needs to create, join, read, or post in a shared Room (room.create, room.add_member, list_rooms, read_room, post_room). Do not use for private mailbox letters, creating Sessions, or talking to AgentBus.
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
`send_message` for private mail (`codeg-mailbox`), `get_session_info`
when the current human turn only pointed at a Session.

Free-text `@alice` never wakes anyone. Use `mention_session_ids`,
`mention_all`, or `mention_human`.

Playbooks (who to create, what to put in `initial_prompt`, when to `@`)
belong to `codeg-multi-agent`. This skill is the Room channel only. A
Session is not a frozen role.

## What an `@` does to the target

An `@` wakes **that Session itself** to read the envelope. It never
creates a substitute, and it never starts a second Session.

- Busy target: the envelope queues and is read when the current Turn
  ends.
- Idle target: the wake is resume-only. The Host resumes the target's own
  runtime; when that Harness cannot resume, the connection is stopped and
  the envelope waits in the target's own queue until it next runs a turn.

Read a silent target as queued, not lost. If a mention draws no reply,
the likely cause is a Harness that cannot resume — the letter is still
there and will be consumed on the Session's next turn. Chase it through
the operator (`mention_human`) rather than re-posting or creating a
replacement Session. Behaviour as of the 2026-08-20 fix
`fix(collaboration): stop a Room @ of an idle Session from creating a
phantom`; before it, `@`ing an idle member spawned a throwaway Session
that answered in the target's place, ran one turn, and was unreachable
from Host Control `session.stop`.

## Lifecycle (Host Control)

Create and membership use Host Control via `codeg_help` / `codeg_use`.
They do not post and they do not send mail.

- `room.create` — create a shared Room. `member_session_ids` takes 1–32
  ids and the caller is added automatically, so one other id already
  makes a two-member Room. The caller is a member like everyone else,
  not an owner.
- `room.add_member` — add an existing Session to a Room you already
  belong to.
- `room.list` / `room.list_workbench` — Workbench-wide list. Prefer
  `list_rooms` when you only need Rooms you already belong to.

Members exist before the Room does. That ordering has a consequence
worth stating plainly: a Session's `initial_prompt` **cannot** carry the
`room_id`, because the Room is created from those Session ids and has no
id until they exist. Do not plan around putting a Room id in a first
prompt. The working order is `session.create` for each member (no
`initial_prompt` needed when the Room will carry the task) →
`room.create` → `room.add_member` for latecomers → post the assignment
and `@` the Session that owns it. The Room post is the brief; unlike a
first prompt it can name the Room, the spec path, and the stop rule.
Observed 2026-08-20.

## Ledger tools

These live on the `codeg-room` MCP server, not `codeg-mcp`. Host Control
`room.create` / `room.add_member` stay on `codeg-mcp`.

- `list_rooms`: Rooms this Session already belongs to. Includes
  `unread_count` (channel posts since you last read),
  `mention_unread_count` (`@` deliveries you have not consumed),
  `needs_reply_count` (you still owe a reply), and
  `awaiting_reply_count` (someone still owes you a reply).
- `read_room`: members plus timeline bodies.
  - Default: newest window. Surrounding context for a mention.
  - `unread=true`: every post since your last-read cursor, including
    posts that did not `@` you. Advances the cursor.
  - `needs_reply=true`: only unpaid reply obligations that involve you
    (you owe, or someone owes you). Does not mark posts read.
  - `before_event_id`: page older.
  - Opening consumes `@` deliveries in the returned window (marks them
    read) unless `needs_reply=true`. It does not clear a reply obligation.
  A Room mention envelope already includes that post's body. If the
  mention quotes another post, the envelope also embeds the parent
  author and a short snippet. Quoting still does not wake that author.
- `post_room`: write to the Room.
  - Omit `mention_session_ids` (and `mention_all=false`) to record only.
  - Pass `mention_session_ids` or `mention_all=true` to tap a Session.
    An archived Session is still named on the timeline; it is not woken,
    and its Delivery comes back `failed (target_archived)` so you see who
    was skipped.
  - Pass `mention_human=true` (or `codeg://human`) to tap the operator.
    That does not wake a Session.
  - `expects_reply` defaults to `false`. Leave it there for anything that
    only records, delivers, or acknowledges — progress notes, "landed,
    here is the path", closing summaries. Set `true` only for a question
    or an assignment you will wait on. The obligation is stamped per
    mentioned Session, so one `expects_reply=true` post to five members
    puts a "needs reply" badge on all five and arms five nag timers; a
    run's worth of receipts sent that way turns every member's badge into
    noise. (Mailbox is the mirror image: `send_message` defaults to
    `true`.) Observed 2026-08-20.
  - File / path text in the body is context, not a Delivery.
  - A reply on the timeline is quoting, not waking. Set
    `reply_to_event_id` to hang on the chain. That does not notify the
    parent author. Wake someone only with a structured mention
    (`mention_session_ids` / `mention_all` / `mention_human`). Reporting
    back to whoever asked you means quoting **and** mentioning them.

## Workflow

1. If you need a new Room, `codeg_use` `room.create`. If you already
   belong to one, call `list_rooms`.
2. The mention envelope already has that post. Call `read_room` when you
   need surrounding posts, a truncated remainder, or `unread=true` to
   catch up on posts you missed (including posts that did not `@` you).
3. Reply with `post_room` and `reply_to_event_id`. Default back to the
   Room, not to private mail. Quoting clears your reply debt; it still
   does not wake the asker. If they must see the answer now, also pass
   `mention_session_ids`. Host will nag if a mention asked for a reply
   and five minutes pass with no linked `post_room`.
4. `@` only the Sessions that must act. Do not `@all` unless asked.

## Threads

Same rule as mail: omitting `reply_to_event_id` starts a new root.
A follow-up on the same Room thread must set it. The envelope JSON
carries `replyToEventId` and, when the parent is known, `parentSnippet`.
Call `read_room` if you need surrounding posts beyond that snippet.

## Hard rules

- You must already be a member to `read_room` or `post_room`.
- Mentions must already be members.
- Quote ≠ wake. `reply_to_event_id` never wakes the parent author.
  Wake only with structured `@`.
- Never answer a Room mention (`channel=room`) with `send_message`.
- Never copy a Room post into a private letter unless asked.
- A `codeg://session/<id>` badge in the current user turn is context,
  not an automatic Room mention. Use `mention_session_ids` (or
  `mention_all`) when you intend to wake someone in this Room.
- If these tools are missing, say Room collaboration is unavailable.
