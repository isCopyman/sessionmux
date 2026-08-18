---
name: codeg-multi-agent
description: Use when a managed Codeg Agent should start or join flexible multi-session collaboration (planner-coder-reviewer, hub-and-spoke, roundtable, or a saved pattern), or persist a pattern after a run. Do not use for mailbox/room protocol, Host Control mechanics, Buzz personas, or Multica squads.
---

# Codeg Multi-Agent

Playbooks for **this run**. The collaboration unit is a Codeg Session, not a
persona. A Session may take different jobs across rooms and days.

This skill does not send mail or post. Channels:

- Sessions and first prompt: `codeg-host-control` (`session.create` with
  `title` + `initial_prompt`)
- Shared ledger: `codeg-room`
- Private letters: `codeg-mailbox`

Read a pattern file before improvising. Product patterns live in
`references/` next to this file. Project-learned patterns live in
`.codeg/collab-patterns/`. User-global learned patterns live in the custom
skill `codeg-collab-patterns` (see `references/persist-and-sync.md`).

## Default move

The human talks to **one** Session (usually this one). That Session
arranges the rest.

1. Reuse existing Sessions (`session.list` / `list_sessions`) before creating.
2. Create only if needed: `session.create` with a this-run `title` and an
   `initial_prompt` that states the job, the Room or mailbox to watch, and
   when to stop. Never invent a harness system prompt. Codeg has no
   per-member system-prompt field; most harnesses will not honor one.
3. Prefer a Room for shared status. `room.create` requires a `title`
   (1–80 chars) and at least one other Session id — the calling Session is
   always added as a member, so passing two other ids makes a three-member
   Room. Everyone is a member; there is no owner.
4. Wake with structured `@` (`mention_session_ids` / `mention_all` /
   `mention_human`). A quote reply is not a wake — reporting back to
   the asker still needs an `@`. Free-text `@alice` never wakes anyone.
5. Use mailbox for a private handoff that should not hit the ledger.
6. `@` only who must act this round. Do not `@all` unless asked.
7. Stop when the pattern's stop rule fires, or when the human says stop.
8. If this Session has a continuation timer and you made progress (or new
   `@` / mail unblocks you), call `timer.reset_delay`. If you are still
   waiting and have nothing else to do, skip it — do not do another
   Session's work just to keep the delay short.

Identity for this run is **title + first prompt + the posts you send**.
Do not freeze a Session as planner/coder/reviewer. Do not build Buzz-style
persona packs, subscription filters, or role names as `@` addresses.

## Pick a pattern

- Sequential design → implement → check: `references/planner-coder-reviewer.md`
- One coordinator, others speak when asked: `references/star.md`
- Several Sessions should see the same thread: `references/roundtable.md`
- Pattern pick, delegation brief, effort scale: `references/patterns-map.md`
- Research / report writing pipeline: `references/research-writing.md`
- Long-form or narrative writing: `references/long-form-writing.md`
- Final convergence meeting on a draft: `references/line-review.md`
- A saved project or user pattern: read that file; fill missing fields from
  `references/pattern-template.md`

If none fit, follow the template for this run. Do not write a new pattern
file until the human confirms (see persist rules).

## Persist a new pattern

After a collaboration that is worth repeating, draft a markdown file from
the template, show it, and wait. On confirmation, write it where
`references/persist-and-sync.md` says — not into this bundled skill, and
not into a harness-private skills copy.
