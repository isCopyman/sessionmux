# Long-form and narrative writing

Novels, serials, and other long narrative pieces. Continuity is a hard
dependency, so the default is a prompt chain, not a fan-out. Everything
lives in files under the shared folder.

## Setup

The hub writes the outline plus setting and character cards as files
(`outline.md`, `cards/*.md`). These cards are the continuity contract —
every later stage checks against them.

## Pipeline

1. Chapters are drafted SEQUENTIALLY — one Session per chapter, or one
   Session for the whole run. Each drafter reads the previous chapter's
   file before writing. Do NOT parallelize chapters: parallel drafters
   cannot see each other's choices in time, and continuity breaks.
2. Continuity review: a reviewer Session checks each new chapter against
   the cards and the prior chapters — timeline, names, established facts,
   promises made to the reader. Findings go back as a NEW letter per round
   (reply chains cap at depth 4).
3. Style unification: after all chapters exist, ONE editor Session makes a
   single voice pass over the full text. Never parallelize this pass.

## Exception: brainstorm

Divergent phases may go wide. For premise, plot forks, or endings, use a
Room roundtable (see `roundtable.md`) or BCC voting — the same question to
N Sessions, the hub compares. Converge again before drafting resumes.

## Wake

Hand-offs are letters carrying the new chapter's path and
`expects_reply=true` (the default). A quote is not a wake — `@` whoever
must act. In a brainstorm Room, `@` only who must speak this round; do not
`mention_all`.

## Stop

The editor posts the final path; the hub posts a closing note and `@human`
for sign-off. Then stop.
