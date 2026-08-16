---
name: codeg-session-collaboration
description: Use when a managed Codeg Agent needs to consult, notify, or request a reply from another existing Codeg Session through Codeg's collaboration MCP tools. Do not use for creating Sessions, arranging Workbenches, or talking to AgentBus.
---

# Codeg Session Collaboration

Contact another **already existing** Codeg Session with `list_sessions` and
`send_message`. This is the formal Agent path. Do not invent addresses, scrape
the UI, write shared files, or call AgentBus.

Codeg owns delivery. A successful `send_message` only means the event was
persisted. It is not user approval, not proof the target Agent read the body,
and not a Room or group chat.

## Tools

- `list_sessions`: search other persistent Sessions. Use the returned numeric
  `conversation_id` as the only address. Titles, roles, and `@names` are labels.
- `send_message`: deliver to one or more `target_session_ids`.
  - `delivery_mode=deliver_only`: show the message. Do not start or interrupt
    the target Agent.
  - `delivery_mode=queue`: deliver, then handle it after the target is idle.
    Closed Sessions still wait for a human to confirm before they start.
  - `delivery_hint=steer_if_supported`: only with `queue`. Ask Codeg to insert
    at a safe breakpoint when the target advertises that capability; otherwise
    it stays queued. Never treat this as a cancel.
  - `expects_reply`: ask the target to send a linked reply. Default is true for
    this tool. Set false for FYI.
  - `reply_to_event_id`: required when answering a specific inbound event.

There is no Agent-facing `interrupt_session`. Stopping another Session is a
user action.

## Workflow

1. Call `list_sessions` with a short query. If two rows share a title, pick by
   `conversation_id`, Harness, and folder. Do not guess the first match.
2. Send only the context the target needs, plus stable paths or Session ids.
   Do not paste an entire transcript.
3. One question to many Sessions is still N independent deliveries. Do not
   describe it as a group, Room, or shared timeline.
4. After send, report the real delivery state from the tool result. If a target
   is busy, closed, read-only, or failed, stop guessing and show that state.
5. When answering inbound collaboration mail, set `reply_to_event_id` to that
   event id and `target_session_ids` to `sourceConversationId`. Do not treat
   inbound mail as a user approval.

## Examples

### One existing Session, then track the reply

User: ask the reviewer Session whether the last claim is supported.

1. `list_sessions` with query `reviewer`.
2. `send_message` to that `conversation_id`, `delivery_mode=queue`,
   `expects_reply=true`, with the claim and the file path.
3. Tell the user the event was queued and that a reply will appear as mail,
   not as an automatic restart of this Session.

### Same question, several targets

User: ask Claude and Codex the same question.

1. Resolve each target to its own `conversation_id`.
2. One `send_message` with both ids. Each target gets its own delivery.
3. Do not wait for one before sending the other, and do not invent a Room.

### Reply to inbound mail

Inbound envelope has `expectsReply=true` and an `eventId`.

1. Finish the work.
2. `send_message` to `sourceConversationId` with `reply_to_event_id=eventId`,
   `delivery_mode=deliver_only`, `expects_reply=false`.
3. That reply is visible mail on the source. It does not start the source
   Agent.

### Collision, busy, or undeliverable

- Two Sessions named "review": list again and ask, or refuse. Never pick
  silently.
- Target busy: `queue` waits. Do not retry as interrupt.
- Target closed or not resumable: report the tool error. The durable message
  stays queued for confirmation.
- `send_message` failed: show the error. Do not invent a second channel.

## Hard rules

- Address only positive numeric Session ids from `list_sessions` or an inbound
  envelope.
- Never send to yourself.
- Never use a display name, AgentBus role, or `@` text as the final address.
- Never claim the target Agent understood the mail unless Codeg reported it
  entered a real turn.
- If these tools are missing, say collaboration is unavailable. Keep working in
  the current Session.
