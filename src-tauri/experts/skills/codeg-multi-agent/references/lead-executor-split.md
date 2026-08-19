# Lead-executor split

One strong-model Session leads; several cheap-model Sessions execute. Use
when the work mixes open-ended judgment with hours of mechanical labor —
audits, migrations, doc overhauls, batch refactors. The economics: the
lead is expensive per token, so it never does bulk work; executors are
cheap, so they never make design calls. The split is scoped to this run,
not a standing org chart.

## Division of labor

The lead owns everything open-ended: decomposition, written specs,
verdicts, review, merges, and every judgment a vague goal requires.
Executors own deterministic packages: apply a spec, run the checks,
commit, report numbers. When a package hits a judgment call mid-flight,
the call climbs to the lead as a facts-only letter; the answer comes back
down as an amended spec. Never let an executor resolve ambiguity by
guessing.

## Staffing

- `session.create` one executor per independent package. Pin `model` to
  the cheap tier in the create call; never staff an executor on the
  lead's expensive model. The `initial_prompt` carries the four-element
  brief (patterns-map.md) plus the path of the committed spec document —
  on conflict, the document wins over the prompt.
- Parallel WRITERS never share one working tree. One worktree or folder
  each; deliverables travel as file paths in letters.

## Lead discipline

- Verify mechanically, not socially. Recompute an executor's claim from
  the artifact itself — rerun the tests, diff the files — before
  accepting the report. Never rely on an executor's memory of what
  changed.
- Before dispatching a package, re-read the target mechanism's recent
  history. A spec written against last week's code sends a worker to
  repaint a wall that was demolished yesterday.
- Take-over protocol: when a package stalls and the lead steps in, first
  send a stand-down letter ("stop, touch nothing shared, report facts"),
  then act. Acting before the stand-down invites a write race on the
  shared tree.
- Two failed retries on one step = stop. The executor reports facts and
  waits; diagnosis of open-ended failures is the lead's job, and cheap
  models burn turns iterating on problems they cannot frame.
- Keep one progress-ledger file in the repo. Every verdict, dispatch and
  landing is written there alongside the work itself. After a context
  compaction the ledger, not anyone's memory, is the source of truth.

## Fleet hygiene

A batch of executors is a batch of sidebar rows. File them from the
start, or the panel is a junk drawer by day two.

- Open ONE Collection per run, named after the task ("auth-migration
  audit", not "executor pool"). File every executor there at creation
  time — `session.create` takes `collection_id`, and
  `collection.add_session` fixes stragglers after the fact. A Session
  has one primary Collection; adding moves it, never copies.
- A big run with separate workstreams can nest one sub-Collection per
  workstream under the run's Collection — Collections form a tree. One
  level is usually enough; a deep tree is as unreadable as a flat pile.
- When an executor's package lands and is verified, archive its Session.
  Archived Sessions keep their transcripts but drop out of the default
  panel view ("Show archived" brings them back), so finished work stops
  competing with live work for the human's attention. Check `codeg_help`
  for an archive action; if none is advertised, archiving is the human's
  panel action — name the finished Sessions in your wrap-up report and
  ask for the archive there.
- Never improvise a hierarchy on the Session `parent_id` field. It is a
  legacy import field from native transcripts, not a filing mechanism.
  The Collection tree is the only place a belongs-to relationship lives.

## Communication

- Assignments and reports are letters with `expects_reply=true` (the
  default); audit outstanding work with `list_inbox` box=sent
  filter=awaiting_reply. Executors routinely finish without reporting —
  the obligation machine chases them so the lead does not have to.
- A report states numbers: commit hashes, tests run/passed, files
  touched, deviations from spec. "Done" without numbers is not a report.
- A Room is optional. Add one only when packages interlock and executors
  must see each other's paths; otherwise BCC keeps their answers
  independent and their context small.
