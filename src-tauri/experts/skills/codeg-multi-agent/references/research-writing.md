# Research writing

Pipeline for papers, reports, and survey-style documents. The draft lives in
files under the shared folder (suggested `docs/rooms/<room-id>/`); letters
and posts carry paths plus one-line summaries, never full text.

## Participants

- Hub: usually the Session the human is talking to. Owns the outline, the
  merge, and the final call.
- Section drafters, reviewers, a fact checker, one consistency editor:
  existing or created Sessions. Labels are this-run only; no Session is
  branded by its job.

## Pipeline

1. Outline: the hub writes `outline.md` — sections, each section's claim,
   and its source boundaries.
2. Section drafts in parallel: brief each drafter by BCC letter (one section
   per drafter; use the delegation brief in `patterns-map.md`). Each drafter
   writes one file per section. BCC keeps drafts independent — nobody
   anchors on a neighbor.
3. Back-to-back review: send the drafts to reviewers by BCC for independent
   problem-finding. Do NOT review in a Room at this stage — hearing each
   other anchors reviewers, and independent signal is the point.
4. Escalate disagreements: when reviewers conflict, open a Room debate on
   exactly that point. Cross-domain conflicts only surface at one table;
   the hub adjudicates and posts the ruling.
5. Merge: the hub merges the section files into one document.
6. Consistency pass: ONE editor Session unifies terminology, notation, and
   voice over the whole document. Never parallelize this pass — two editors
   re-introduce the seams you are removing.
7. Citation and fact check: an independent checker verifies claims and
   citations against sources, evaluator-optimizer style. Each round is a
   NEW letter quoting the previous round's event id or file version —
   reply chains cap at depth 4.
8. Final line review: converge segment by segment in a Room meeting.
   See `line-review.md`.

## Wake

Assignment and review letters carry `expects_reply=true` (the default); the
obligation machine chases late answers. A quote alone never wakes anyone —
`@` with `mention_session_ids` whoever must act this round.

## Stop

The hub posts the final path plus a one-paragraph abstract and `@human` for
sign-off. Then stop.
