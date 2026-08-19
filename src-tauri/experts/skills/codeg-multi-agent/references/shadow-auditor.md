# Shadow auditor

One read-only Session re-checks a key executor's real output — the diff,
the test run, the decisions — and reports doubts to the lead, never to
the executor. Use it as an add-on to lead-executor-split.md: the lead
still leads, the executor still executes, the auditor only watches.

The auditor exists because the lead's "verify mechanically" discipline
has a blind spot: the lead wrote the spec, so the lead reads the diff
through the spec's assumptions. A second pair of eyes that never saw the
spec's reasoning catches what the author's eyes slide over.

## When it pays

Staff an auditor when a wrong landing costs more than a second Session:

- **High-risk changes.** Auth, payments, migrations, deletes. One bad
  merge here undoes the economics of the whole cheap-executor run.
- **Mechanical batch changes.** A codemod that is wrong in the same way
  40 times looks wonderfully consistent in a spot check. The auditor's
  job is to check the *pattern*, not the count: "files touched: 41" is
  a number; "all 41 wrong the same way" is a disaster with good
  paperwork.
- **The executor is a cheap model.** Cheap models are confident when
  wrong. If the executor's tier was chosen for price, buy back some
  reliability with audit instead of a pricier executor.

Skip it for small, reversible chores. Auditing doubles the coordination
cost of a package; a package that takes an hour and can be reverted with
one `git revert` does not need a watcher.

## Setup

1. `session.create` the auditor with a title like "audit: auth-migration"
   and an `initial_prompt` that states the job in plain words, e.g.:

   > You are the read-only auditor for the auth-migration run. Executor
   > "ex-3" applies spec docs/specs/auth-migration.md. Each time it
   > reports a package done, re-verify from the artifacts: read the diff
   > yourself, rerun the tests yourself, compare against the spec
   > document. Never edit files, never run write commands, never message
   > the executor. Send every doubt to me (the lead) by letter, with
   > file:line evidence. If the evidence does not settle the question,
   > say so instead of guessing.

   The prompt is the whole enforcement — Codeg has no read-only Session
   flag, so say "read-only" in the first prompt and mean it.
2. Give the auditor eyes, not just letters. Either add it to the same
   Room so it sees executor reports as they land, or keep it off the
   Room and have the lead forward each report by mailbox. The Room is
   less relay work; the mailbox keeps the auditor un-anchored by room
   chatter. For one auditor on one executor, the Room is usually fine.
3. The auditor works in the same cwd, so it can `git diff` and rerun
   tests on its own. Point it at the spec document path and the
   progress ledger in the first prompt — it audits against documents,
   not against anyone's memory.

## Cadence

One audit round per completion report. The natural trigger: the
executor's "done, here are my numbers" lands, the lead `@`s the auditor
quoting that report, the auditor answers with one of two shapes:

- **Clean:** "diff matches spec §2–3, tests 38/38 locally, no
  deviations." One line, with what was actually checked.
- **Doubt:** "spec §3 says drop the legacy column; diff keeps it in
  `models/user.rs:88`. Tests pass because the fixture never exercises
  it." Doubts come with file:line and a reproduction, or they are noise.

Mid-flight audits are wasted motion. Auditing a half-applied package
produces doubts about states the executor already plans to fix.

## Escalation: doubts climb, never sideways

The auditor's doubts go to the **lead**, and the lead rules. This is the
load-bearing rule of the pattern. If the auditor messages the executor
directly, the executor now has two bosses with possibly different
readings of the spec, and it will average them into something neither
wanted.

A worked example:

1. Executor (in Room): "Package 4 done. 41 files migrated, tests
   38/38, commit a1b2c3d."
2. Lead: "@auditor package 4, go."
3. Auditor (letter to lead): "Not clean. `auth/session.rs:142` still
   calls the old token path — the codemod missed it because the call
   site is inside a macro. Pattern is wrong, not the count: grep finds
   5 more macro-wrapped call sites it would also miss."
4. Lead rules. If the doubt stands, the executor gets an amended spec
   ("extend the pattern to macro call sites, here are the 6 locations"),
   not the auditor's raw letter. If the lead overrules, the reason goes
   in the progress ledger so the auditor can see the call and calibrate.

The auditor that keeps losing rulings on the same kind of doubt should
say less about that kind; the auditor that keeps winning should have its
doubts promoted into spec checklist items. Both adjustments are the
lead's, written in the ledger.

## Boundaries

- **Never edits code.** Not "small fixes", not "just this once". An
  auditor that fixes is an uncoordinated second executor, and its fixes
  are unaudited.
- **Never instructs the executor.** No hints, no nudges, no "have you
  considered". All direction flows lead → executor; all doubt flows
  auditor → lead.
- **Never blocks on its own authority.** A doubt does not stop the line;
  the lead's ruling does. The auditor states evidence and confidence,
  then gets out of the way.
- **When the evidence does not settle it, escalate to the human.** "The
  diff is safe iff the old tokens never outlive the deploy, and I cannot
  verify that from the repo" is a complete and honorable audit result.
  The lead passes it up with the facts; guessing to avoid bothering the
  human is the one failure mode worse than a missed bug.
