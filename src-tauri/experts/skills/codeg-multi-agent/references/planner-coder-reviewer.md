# Planner–coder–reviewer

## Scenario
One pass of design, implementation, then review. Use when the human wants a
split of those jobs **this run**. Do not use this to permanently brand
Sessions as planner/coder/reviewer.

## Participants
Reuse existing Sessions when they already have the repo context.

Typical this-run labels (titles only):

- Planner: scope, files, risks, stop conditions. Does not implement unless
  asked.
- Coder: implements against the plan. Does not merge review into a new
  design pass unless asked.
- Reviewer: defects and gaps only. Does not rewrite the feature unless asked.

The initiating Session may **be** one of these (often planner) instead of
creating a fourth Session.

The Room cannot be named in an `initial_prompt`: `room.create` consumes
the Session ids, so the Room is younger than its members. Create the
Sessions first (an `initial_prompt` is optional and often better left
off), then `room.create`, then post the job, the Room-visible plan path
and the stop rule as a Room post that `@`s the Session that owns it.
No system prompt.

Delegation template & effort scaling: see `patterns-map.md`.

## Channels
1. Optional mailbox from the initiator to planner with the human's goal.
2. Room for the shared plan, implementation notes, and review.
3. Mailbox only for a private nit that should not clutter the ledger.

Put the plan on the Room so coder and reviewer read the same text.

## Wake
- After the plan is posted: `@` coder (and reviewer only if they must read
  the plan now).
- After implementation notes: `@` reviewer.
- After review: `@` coder for blocking findings, or `@` the initiator /
  `mention_human` when the run should stop.

Do not `@all` on every status line.

## Stop
Reviewer posts a closing note: approve, or a finite findings list. Coder
fixes blocking items and `@` reviewer once. Then stop unless the human
asks for another pass. Do not start a second planner turn unprompted.
