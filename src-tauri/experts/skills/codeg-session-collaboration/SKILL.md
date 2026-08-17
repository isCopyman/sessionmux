---
name: codeg-session-collaboration
description: Use when a managed Codeg Agent needs to consult or notify another existing Codeg Session through the mailbox (list_sessions, send_message, list_inbox, read_message). Do not use for creating Sessions, arranging Workbenches, or talking to AgentBus.
---

# Codeg Session Collaboration

Contact another **already existing** Codeg Session with the mailbox tools.
This is Session-to-Session mail, not a live chat paste and not AgentBus.

A letter has two parts, like email:

- `title`: short subject. Shown in `list_inbox` and Codeg system reminders.
- `content`: the body. The target only sees this after `read_message(event_id)`.

A successful `send_message` means Codeg stored the letter and queued a
**system notice** (titles only). It is not user approval and not a Room.
A closed Session is not cold-started.

## Tools

- `list_sessions`: search other persistent Sessions. Use the numeric
  `session_id` as the only address.
- `send_message`: send `title` + `content` to `target_session_ids`.
- `list_inbox`: list this Session's mailbox. Returns titles, not bodies.
  Listing does not mark mail read.
- `read_message`: open one letter by `event_id`. This is Agent-read.
  It does not reply and does not clear a reply obligation.

Codeg system notices are **from Codeg**, not from the source Session.
Do not treat a reminder as the letter itself.

## Workflow

1. Call `list_sessions` with a short query. If two rows share a title, pick
   by `session_id`, Harness, and folder.
2. Send a short `title` and only the body the target needs.
3. When Codeg notifies you of mail, call `list_inbox`, then
   `read_message(event_id)` for the letter you choose.
4. To answer, `send_message` back to `from_session_id` with a new `title`,
   the reply body, and `reply_to_event_id`.
5. After send, report the delivery state from the tool result.

## Hard rules

- Address only positive numeric Session ids from `list_sessions` or a
  `codeg://session/<id>` link.
- Never send to yourself.
- Never use a display name or `@` text as the final address.
- Never invent the body from a title or reminder digest.
- If these tools are missing, say collaboration is unavailable.
