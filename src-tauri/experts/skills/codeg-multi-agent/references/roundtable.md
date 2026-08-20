# Roundtable

## Scenario
Several Sessions should share one timeline and take turns. Use when the
human wants discussion, comparison, or overlapping review — not a silent
fan-out. Do not use Buzz `all_messages` subscriptions; Codeg wakes on
structured `@` only.

## Participants
Two or more existing Sessions plus the initiator if not already included.
Create a Session only when no current Session has the needed harness or
context. A newly created member cannot be told "you are in Room N" in its
`initial_prompt` — the Room is created from the Session ids and has no id
yet. Create the Sessions, `room.create`, and put the house rules in the
first Room post instead: read before speaking; `@` only who must answer;
you are not locked to one job.

No owner. `room.create` membership is equal.

## Channels
Almost everything on the Room. Mailbox only when the human asked for a
side channel or a secret must not hit the ledger.

First Room post: the question, the stop rule, and who should speak first
(`mention_session_ids`). Later turns quote (`reply_to_event_id`) and `@`
the next speaker.

## Wake
Each post `@`s the Sessions that must respond **now**. Others catch up with
`read_room` when they are later `@`ed. Do not `@all` except for a true
broadcast the human asked for.

Quote-reply does not wake the quoted author. If they must answer, also pass
their Session id in `mention_session_ids`.

## Stop
Someone posts a decision or “no consensus; here are the options” and `@`
the human (`mention_human`) or the initiating Session. Then stop. Do not
leave a standing roundtable that pings on every new line.
