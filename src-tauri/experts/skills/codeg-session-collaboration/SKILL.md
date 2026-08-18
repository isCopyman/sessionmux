---
name: codeg-session-collaboration
description: Use when a managed Codeg Agent needs to consult or notify another existing Codeg Session through the private mailbox (list_sessions, send_message, list_inbox, read_message). Do not use for shared Rooms, creating Sessions, arranging Workbenches, or talking to AgentBus.
---

# Codeg Session Collaboration

Contact another **already existing** Codeg Session with the **mailbox**
tools. This is private mail. It is not a Room post.

A letter has two parts, like email:

- `title`: short subject. Shown in `list_inbox` and Codeg system reminders.
- `content`: the body. The target only sees this after
  `read_message(event_id)`.

A successful `send_message` means Codeg stored a private letter.
`priority=high` (default) notifies now: if the target is working, Codeg
steers a notice into the current turn when that channel exists,
otherwise it stops the turn and delivers. `priority=normal` waits for
the target's next turn. Both are Agent mail. It is not user approval
and not a Room.

## Tools

- `list_sessions`: search other persistent Sessions. Use the numeric
  `session_id` as the only address.
- `send_message`: send `title` + `content` to `target_session_ids`.
  Recipients cannot see each other. `priority=high` (default) notifies
  now; `priority=normal` waits for the next ordinary turn. Reply or
  supplement with `reply_to_event_id`. Never pass `room_id`.
- `list_inbox`: this Session's private mailbox. Room posts never appear
  here. Returns titles, not bodies. Listing does not mark mail read.
- `read_message`: open one letter by `event_id`.

Shared discussion belongs to the `codeg-room` skill (`list_rooms`,
`read_room`, `post_room`). Host Control `room.create` / `room.add_member`
creates membership; it does not send mail.

## Workflow

1. Call `list_sessions` with a short query. If two rows share a title, pick
   by `session_id`, Harness, and folder.
2. Private question: send a short `title` and only the body the target needs.
3. When Codeg notifies you of **private** mail, call `list_inbox`, then
   `read_message(event_id)`.
4. After send, report the delivery state from the tool result.

## Threads

`reply_to_event_id` is how a letter stays on a thread:

- No `reply_to_event_id` = a new root, a new thread. The earlier exchange
  will not continue in the mailbox UI.
- A first reply, and any later supplement (“I forgot one thing”), must set
  `reply_to_event_id` to the letter you are continuing: the inbound
  `event_id`, or the `event_id` you just sent in that thread.
- Multiple letters may hang off the same parent. The first linked reply
  clears `expects_reply` debt; later supplements stay on the thread and do
  not reopen the debt unless you set `expects_reply` again.

Never omit `reply_to_event_id` just because you already answered once.

## Hard rules

- Address only positive numeric Session ids from `list_sessions` or a
  `codeg://session/<id>` link. A badge in the current user turn is
  context for `get_session_info` unless the user asked you to write mail.
- Never send a private letter to yourself.
- Never use a display name or `@` text as the final address.
- Never invent the body from a title or reminder digest.
- Never pass `room_id` to `send_message`. Rooms use `post_room`.
- If these tools are missing, say collaboration is unavailable.
