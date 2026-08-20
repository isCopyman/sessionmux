# Hub and spoke (star)

## Scenario
One Session coordinates; others work on separate slices and report back to
the hub. Use when tasks are independent enough that spokes should not debate
each other on the ledger. Do not use this to recreate a Multica squad
(work does not silently route only to a leader).

## Participants
- Hub: usually the Session the human is talking to. Arranges members, posts
  assignments, collects results.
- Spokes: existing or newly created Sessions. A new spoke's
  `initial_prompt` cannot name the Room — `room.create` needs the spoke
  ids first, so the Room does not exist yet. Create the spokes, create the
  Room, then give each spoke **one** slice in a Room post that `@`s it:
  “reply in the Room by quoting the assignment; `@` the hub when done; do
  not `@` other spokes unless the hub asked.”

Titles are this-run labels. A spoke may be a hub in another Room.

Delegation template & effort scaling: see `patterns-map.md`.

## Channels
Room is the shared board. Hub posts assignments with structured `@` of the
spoke that owns that slice. Spokes post results on the same thread
(`reply_to_event_id`) **and** `@` the hub. A quote alone does not wake
the hub.

Mailbox: hub → one spoke for a private brief, or spoke → hub for a private
blocker. Do not fan out the same private letter as a substitute for the Room.

## Wake
Hub `@`s only the spoke that must act. A spoke `@`s the hub (or
`mention_human` if the hub is not a Session). Spokes do not `@` each other
by default.

## Stop
Hub posts a closing summary when every slice has a result or a recorded
blocker. Then stop. Do not keep the star alive as a standing org chart.
