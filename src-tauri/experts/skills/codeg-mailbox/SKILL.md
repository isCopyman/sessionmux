---
name: codeg-mailbox
description: Use when a managed Codeg Agent needs to consult or notify another existing Codeg Session through the private mailbox (list_sessions, send_message, list_inbox, read_message). Do not use for shared Rooms, creating Sessions, arranging Workbenches, or talking to AgentBus.
---

# Codeg Mailbox

Contact another **already existing** Codeg Session with the **mailbox**
tools on the `codeg-mailbox` MCP server. This is private mail. It is not a
Room post, and these tools do not appear on `codeg-mcp` or `codeg-room`.

A letter has two parts, like email:

- `title`: short subject. Shown in `list_inbox` and Codeg system reminders.
- `content`: the body. The first delivery copies it into the target's
  prompt (truncated if long). `read_message(event_id)` marks the letter
  read and returns the full body. Listing inbox does not mark it read.

A successful `send_message` means Codeg stored a private letter.
`priority=high` (default) notifies now: if the target is working, Codeg
steers the letter into the current turn when that channel exists,
otherwise it stops the turn and delivers. `priority=normal` waits for
the target's next turn. Both are Agent mail. It is not user approval
and not a Room.

## Tools

- `list_sessions`: search other persistent Sessions. Use the numeric
  `session_id` as the only address.
- `send_message`: send `title` + `content` to `target_session_ids`.
  Recipients cannot see each other. `priority=high` (default) notifies
  now; `priority=normal` waits for the next ordinary turn. Reply or
  supplement with `reply_to_event_id`. A mailbox reply is still a
  letter to those targets, so they are notified — unlike a Room quote,
  which does not wake anyone unless you also `@` them. Never pass
  `room_id`.
- `list_inbox`: this Session's private mailbox. Room posts never appear
  here. Returns titles, not bodies. Listing does not mark mail read.
- `read_message`: open one letter by `event_id`. Marks it read even if
  the first delivery already showed the body.

Shared discussion belongs to the `codeg-room` skill (`list_rooms`,
`read_room`, `post_room`, plus Host Control `room.create` /
`room.add_member`). Creating a Room does not send mail.

How to start a team, pick planner-coder-reviewer / hub-and-spoke /
roundtable, or save a pattern belongs to `codeg-multi-agent`. This
skill is the private-mail channel only.

## Workflow

1. Call `list_sessions` with a short query. If two rows share a title, pick
   by `session_id`, Harness, and folder.
2. Private question: send a short `title` and only the body the target needs.
3. When Codeg delivers **private** mail, the envelope JSON has
   `"channel":"mailbox"` and `"kind":"letter"`. The host line is
   `channel=mailbox`. That is not a Room mention (`channel=room` /
   `kind=room_mention`). Answer mailbox with `send_message`; answer a
   Room @ with `post_room`. A `codeg://session/<id>` badge is only an
   address — it does not choose the channel.
   Call `read_message(event_id)` to mark it read. Use `list_inbox` if you
   need titles you did not consume. Time-out reminders are title-only; they
   do not repeat the body.
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
- A Room mention envelope (`channel=room`) is not inbox mail. Do not
  answer it with this skill's tools.
- If these tools are missing, say the mailbox is unavailable.
