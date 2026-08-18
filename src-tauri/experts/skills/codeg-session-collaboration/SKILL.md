---
name: codeg-session-collaboration
description: Use when a managed Codeg Agent needs to consult or notify another existing Codeg Session through the mailbox, or post in a shared Room (list_sessions, send_message, list_inbox, read_message, plus Host Control room.*). Do not use for creating Sessions, arranging Workbenches, or talking to AgentBus.
---

# Codeg Session Collaboration

Contact another **already existing** Codeg Session with the mailbox tools,
or post in a **Room** when the work is shared.

A letter has two parts, like email:

- `title`: short subject. Shown in `list_inbox` and Codeg system reminders.
- `content`: the body. For private mail, the target only sees this after
  `read_message(event_id)`. Room posts are visible to every member on the
  Room timeline; only `@` mentions create a delivery.

A successful `send_message` without `room_id` means Codeg stored a private
letter and queued a **system notice** (titles only) for the target Agent.
Every private letter is delivered; there is no display-only / deliver_only
mode. It is not user approval and not a Room.

A successful `send_message` with `room_id` means Codeg stored a Room-visible
event. Empty `target_session_ids` is record-only (nobody is woken). Mention
ids still create deliveries.

## Tools

- `list_sessions`: search other persistent Sessions. Use the numeric
  `session_id` as the only address.
- `send_message`: send `title` + `content`.
  - Private mail: `target_session_ids` required, omit `room_id`.
  - Room post: set `room_id`. Mentions go in `target_session_ids` or
    `mention_all=true`. Reply or supplement with the same `room_id` and
    `reply_to_event_id`.
- `list_inbox`: this Session's **private** mailbox. Room posts never appear
  here. Returns titles, not bodies. Listing does not mark mail read.
- `read_message`: open one letter or Room mention by `event_id`.
- Host Control (via `codeg_help` / `codeg_use` when available):
  - `room.list` — list Rooms on a Workbench
  - `room.create` — create a Room; you become owner
  - `room.add_member` — add a Session you already share a Room with
  - `room.post` — record-only / store-only Room post (does not wake)

## Workflow

1. Call `list_sessions` with a short query. If two rows share a title, pick
   by `session_id`, Harness, and folder.
2. Private question: send a short `title` and only the body the target needs.
3. Shared discussion: `room.create` (or `room.list`), then `send_message`
   with `room_id`. `@` only the Sessions that must act.
4. When Codeg notifies you of **private** mail, call `list_inbox`, then
   `read_message(event_id)`.
5. When Codeg notifies you of a **Room mention**, call `read_message` and
   reply in the same Room. Do not send a private letter unless asked.
6. After send, report the delivery state from the tool result.

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
  `codeg://session/<id>` link.
- Never send a private letter to yourself.
- Never use a display name or `@` text as the final address.
- Never invent the body from a title or reminder digest.
- Room and mailbox are separate. A Room post must not be answered as
  private mail.
- If these tools are missing, say collaboration is unavailable.
