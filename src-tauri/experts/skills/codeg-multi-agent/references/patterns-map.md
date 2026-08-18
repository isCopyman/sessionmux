# Patterns map

Anthropic-style agent patterns, mapped to Codeg mechanisms. Read this before
inventing a coordination scheme — the existing base carries all of them.

## Pattern → mechanism

- Prompt chain (sequential steps): one Session per step, mailbox relays the
  artifact. Each letter carries the file path plus a one-line summary, never
  the full text.
- Routing: the hub picks a specialist with `list_sessions` /
  `get_session_info`, then delivers the brief by letter.
- Parallel sectioning: one letter, several `target_session_ids` (≤16).
  Mailbox is BCC — recipients cannot see each other, which keeps answers
  independent.
- Parallel voting: send the same question to N Sessions by BCC and compare
  the answers yourself. On disagreement, open a Room and let the dissenters
  argue where everyone can hear.
- Orchestrator-workers: `session.create` (with `initial_prompt`) to staff,
  mailbox to assign, a Room as the shared board. See `star.md`.
- Evaluator-optimizer: drafter and reviewer as two Sessions exchanging
  rounds. Obey the chain-depth rule below.
- Autonomous long run: one Session plus a continuation timer
  (`timer.reset_delay` after real progress), with `ask_user_question` as
  the checkpoint.

## Delegation brief — four elements

Every assignment (letter or `initial_prompt`) states all four:

1. Objective — the one outcome this worker owns.
2. Deliverable format — which file, what shape, what "done" means.
3. Tool and source guidance — what to read, what to trust, what to ignore.
4. Boundaries — what not to touch, who owns the neighboring slice.

Skipping element 3 or 4 is how two workers redo each other's work.

## Effort scale

- Simple fact or lookup: one Session, few steps. Do not staff a team.
- Comparison or a handful of angles: 2–4 Sessions.
- Complex research: several Sessions, one domain each; the hub merges.
- More Sessions is not more rigor. Add one only when a slice is truly
  independent.

## Artifacts live in files

Deliverables go to files under the shared folder — suggested
`docs/rooms/<room-id>/`. Letters and posts carry the path plus a one-line
summary, never the full text. A hand-off is: write the file, post the path,
`@` the hub. This survives context compaction and kills the telephone game.

## Review rounds: new letter per round

Agent reply chains cap at depth 4 — that fuse is deliberate. Each review
ROUND is a NEW letter (or root post) that quotes the previous round's event
id or artifact path. Never run an ever-deepening reply thread; past the cap
a reply obligation can no longer be attached.

## Field lessons

- Done is not delivered. Workers finish and forget to report. Send every
  task letter with `expects_reply=true` (the default) and trust the
  obligation machine — unpaid replies get chased automatically. Audit with
  `list_inbox` box=sent, filter=awaiting_reply.
- Changed your mind? Use Host Control `session.cancel_turn`; do not just
  send another letter. The old instruction may already be running and the
  worker may not check mail mid-turn.
- Parallel coding on one shared tree is the biggest accident surface.
  Isolate each worker in its own worktree or folder; never share one dirty
  tree. Deliver via files and paths.
