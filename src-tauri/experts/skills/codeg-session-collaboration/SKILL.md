---
name: codeg-session-collaboration
description: Use when a managed Codeg Agent needs to consult or notify another existing Codeg Session with list_sessions and send_message. Do not use for creating Sessions, arranging Workbenches, or talking to AgentBus.
---

# Codeg Session Collaboration

Contact another **already existing** Codeg Session with `list_sessions` and
`send_message`. This is Session-to-Session messaging, not a mailbox. Do not
invent addresses, scrape the UI, write shared files, or call AgentBus.

A successful `send_message` means Codeg queued the letter like a human
follow-up: it steers into a busy turn when the target supports that, otherwise
it starts a new turn. A closed Session is not cold-started; the letter runs
when that Session reconnects. It is not user approval and not a Room.

## Tools

- `list_sessions`: search other persistent Sessions. Use the returned numeric
  `session_id` as the only address. Titles, roles, and `@names` are labels.
- `send_message`: send `content` to one or more `target_session_ids`.
  Codeg chooses the path: steer into the current turn when that exists,
  otherwise start a new turn. A closed Session is not cold-started.

Creating Sessions or Collections uses `codeg_help` / `codeg_use`, not these
tools.

## Workflow

1. Call `list_sessions` with a short query. If two rows share a title, pick by
   `session_id`, Harness, and folder. Do not guess the first match.
2. Send only the context the target needs. Do not paste an entire transcript.
3. One question to many Sessions is still N independent messages.
4. After send, report the delivery state from the tool result.
5. To answer, `send_message` back to the source Session. There is no reply
   obligation or inbox tool.

## Hard rules

- Address only positive numeric Session ids from `list_sessions` or a
  `codeg://session/<id>` link.
- Never send to yourself.
- Never use a display name or `@` text as the final address.
- If these tools are missing, say collaboration is unavailable.
