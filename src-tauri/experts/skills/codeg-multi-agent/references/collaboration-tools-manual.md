# Collaboration tools manual

Tool-by-tool operation manual for a managed Session: what each collaboration
tool does, how to fill its parameters, what comes back, and which neighbor to
pick instead. Patterns and orchestration (who to create, how to split work)
live in the sibling files — see `patterns-map.md` first. Channel overviews
live in the `codeg-mailbox`, `codeg-room`, and `codeg-host-control` skills;
this file is the consolidated operator reference.

Everything here describes the `codeg-mcp` companion surface. Which tools you
actually see depends on the feature groups this launch was given: a tool
outside your groups is hidden from `tools/list` and rejected as
`unknown tool` if called anyway. Mailbox and Room always travel as their own
MCP servers (`codeg-mailbox`, `codeg-room`), so `send_message` and
`post_room` never appear in the same tool list.

## Addressing and identity

- Sessions are addressed by their numeric Codeg Session id — from
  `list_sessions`, a Room roster, or a `codeg://session/<id>` link. Never
  address by display name, and never invent an id.
- A `codeg://session/<id>` badge in the human's message is a pointer, not a
  letter. Read it with `get_session_info`; write mail or a Room post only if
  asked.
- Caller identity is derived from the companion token. Never pass `from`,
  `source_session_id`, `current_session_id`, a token, or a request id —
  Host Control rejects them, and the idempotency key is minted outside
  model-controlled arguments.
- `room_id` is a string; `session_id` is an integer. Do not mix them.

## Mailbox — private letters (`codeg-mailbox`)

Private Agent-to-Agent mail. Recipients cannot see each other (BCC); there
is no CC. Envelopes arrive marked `channel=mailbox`, `kind=letter`.

### list_sessions

Find other persistent Sessions to address. Excludes you and unsent drafts;
returns title, Harness, workspace, status, last activity.

- `query` — optional case-insensitive filter over title, id, Harness,
  workspace. Narrow the query instead of raising `limit` (default 50,
  max 200).
- Returns rows to pick a `session_id` from. When titles collide,
  disambiguate by Harness and workspace.
- vs Host Control `session.list`: that one is project-scoped management
  metadata; this one is the mail address book.

### send_message

Send a durable private letter.

- `target_session_ids` — 1–16 ids. BCC: recipients cannot see each other.
- `title` — 1–120 chars, subject-line style. Shown in `list_inbox` and in
  reminders; the body never is. Do not cram the message into the title.
- `content` — the body. The first delivery copies it into the target's
  prompt, truncated past 8000 chars; the target calls `read_message` for
  the rest.
- `priority` — `high` (default) notifies now: steer into the running turn
  when the channel exists, else deliver when the current turn finishes;
  a closed Session is started. `high` never interrupts a turn. `normal`
  waits for the next ordinary turn.
- `expects_reply` — defaults to `true`. Keep it `true` for anything that
  assigns work; set `false` only for FYI.
- `reply_to_event_id` — set it for the first reply and every later
  supplement on a thread, or you start a new root. First linked reply
  clears the reply debt; supplements do not reopen it.
- Passing `room_id` or `mention_all` is rejected with a hint — that is
  `post_room` territory.
- An archived target rejects the whole send, and the note names the
  archived Session. Restore with Host Control `session.unarchive` or pick
  another peer.
- Returns the new `event_id` plus a per-target delivery state line.
  Delivery or queueing is not completion — a failed delivery names its
  reason (e.g. `target_archived`).

### list_inbox

Titles only, newest first. Listing never marks anything read.

- `box` — `inbox` (default) for received, `sent` for your outbox.
- `filter` — `open` (default: unread or awaiting reply), `unread`,
  `awaiting_reply`, `all`. The audit combo is `box=sent` +
  `filter=awaiting_reply`: who still owes you.
- `peer_session_id` — narrow to one correspondent.

### read_message

Open one inbound letter by `event_id`; returns title and full body and
marks it read (reminders stop). It does not reply and does not clear a
reply obligation — answer with `send_message` to `from_session_id` and
`reply_to_event_id` set. Never invent an event id; they come from
`list_inbox` or an envelope.

## Room — the shared ledger (`codeg-room`)

A Room is a shared timeline every member can read. Envelopes arrive marked
`channel=room`, `kind=room_mention`, and already carry the post body plus
a short parent snippet when the mention quotes another post.

### list_rooms

Rooms you already belong to, with `unread_count`, `mention_unread_count`,
`needs_reply_count` (you owe), `awaiting_reply_count` (you are owed).
Use it to find a `room_id`. For a Workbench-wide list of Rooms you have
not joined, use Host Control `room.list`.

### read_room

Read the timeline of a Room you belong to.

- Default: newest window (`limit` default 50, max 200).
- `unread=true` — everything since your last-read cursor, oldest first,
  including posts that never `@`ed you. Advances the cursor.
- `needs_reply=true` — only unpaid reply obligations involving you. Does
  not mark posts read, and takes precedence over `unread` and
  `before_event_id`.
- `before_event_id` — page older.
- Opening consumes the `@` deliveries in the returned window unless
  `needs_reply=true`. It never clears a reply obligation — only a linked
  `post_room` reply does.

### post_room

One post, no title; the body is the whole post.

- Omit all mention fields for a record-only post that wakes nobody.
- `mention_session_ids` (≤16, members only) — the only way to wake
  specific Sessions. An archived member stays named on the timeline but
  its delivery comes back `failed (target_archived)`.
- `mention_all` — every other Session member. Never includes the human.
  Use only when `@all` is genuinely intended.
- `mention_human` — tap the operator. See the @human discipline below.
- `priority` — `high` by default when anyone is mentioned; `normal` waits
  for the next ordinary turn. Ignored on record-only posts.
- `expects_reply` — defaults to `false`; set `true` when the post assigns
  work.
- `reply_to_event_id` — quotes the parent on the timeline. Quoting is not
  waking: reporting back to the asker means quoting AND mentioning them.
- Free-text `@alice` in the body wakes no one. File paths in the body are
  context, not deliveries.
- Returns the post's `event_id` and per-mention delivery states.

### Room lifecycle (Host Control, on `codeg-mcp`)

- `room.create` — `title` (1–80 chars) and `member_session_ids` (1–32).
  You are always a member, so one other id makes a two-member Room. There
  is no owner. Post afterwards with `post_room`, never `send_message`.
- `room.add_member` — `room_id` + `session_id`; you must already belong.
- `room.list` / `room.list_workbench` — Workbench-wide read
  (`workbench_id` defaults to 1). Prefer `list_rooms` for Rooms you
  joined.
- `room.post` no longer exists; the gateway rejects it with a migration
  hint to `post_room`.

## Host Control gateway (`codeg-mcp`)

Two tools front a server-owned action catalog. Always discover before use.

### codeg_help

Read-only catalog query. `query` filters by words, `action` pins one exact
id (e.g. `session.rename`); omit both for the full current catalog. The
catalog is filtered by live policy: when writes are disabled you see only
the read actions. If an action is not advertised, it is unavailable —
report that instead of improvising another channel. Returns schemas, not
object ids.

### codeg_use

Execute one action: `action` plus an `input` object matching the advertised
schema exactly — unknown fields are rejected. Outcomes carry a `stage`:
`read`, `persisted`, `ui_requested`, or a `rejected` note (there is no
`not_found` stage; a missing target comes back `rejected` naming it).
`replayed: true` means the same call was already applied — safe to retry a
call whose response you lost, but after a Host restart, list current state
before repeating a create.

### Session actions

- `session.list` / `session.get` — project-scoped metadata (title, agent,
  status, model, message count, archived flag). `session.list` excludes
  you. `session.get` never exposes another Session's transcript.
- `session.rename` — persist a manual title (omit `session_id` to rename
  yourself).
- `session.create` — the only way to staff a Session. `harness` is
  required (e.g. `codex`, `claude_code`); `folder_id`/`cwd` outside your
  own scope are rejected. `title` + `initial_prompt` carry this-run
  identity — the prompt is a user message, not a system prompt; there is
  no system-prompt field. `model` and `config_values` are verified against
  what the Harness advertises. `collection_id` files the new Session on
  creation; a placement failure keeps the Session. Watch the result
  `stage` — `created`, `turn_started`, and the failure stages are
  distinct outcomes.
- `session.cancel_turn` — cancel only the running Turn; the Session and
  runtime survive. Idle targets return an explicit no-op stage. This is
  the brake pedal when instructions change mid-flight — see Delivery
  discipline.
- `session.stop` — additionally disconnect the runtime. Identity,
  transcript, and resume are preserved; nothing is deleted or archived.
- `session.get_selectors` / `session.set_selectors` — read, then pin,
  model and/or thinking effort (at least one required). The pin persists
  across restarts and wins over the user's saved defaults. Effort
  vocabularies differ per Harness — always read the advertised choices
  first; without a live runtime the value is pinned unchecked.
- `session.archive` / `session.unarchive` — hide or restore. An archived
  Session refuses new mail (the sender is told) and Room `@`s to it fail
  as `target_archived`; transcript and runtime are kept. Archiving waives
  every reply owed TO it, finally — restoring does not revive them.

### Organization actions

- `collection.list/create/rename/move` — the Collection tree in your Path
  scope (`parent_id` omitted or null = root).
- `collection.add_session` — set a Session's ONE primary Collection;
  adding moves, never copies. `collection.remove_session` leaves it
  Unclassified. Neither touches cwd or runtime.
- `workbench.list/create/rename` — the app-wide saved Workbench namespace;
  memberships outside your Path are redacted. Create/rename never mount,
  switch, or focus.
- `workbench.add_session` / `workbench.remove_session` — persist or drop a
  Session reference; no UI side effects.
- `workbench.place_session` — the one UI-affecting action: open and focus
  a Session, optionally split right/down. Use only when the human asked
  for layout. A `ui_requested` stage is not proof a disconnected view
  applied it.

### Timer actions

Continuation timers keep THIS Session advancing after each completed Turn
(detailed loop guidance: the `codeg-session-timer` skill). All timer
actions operate on the current Session's own timers.

- `timer.create` — enqueue `prompt` as the next ordinary follow-up after
  every completed Turn. `idle_grace_seconds` is a debounce, not a
  wall-clock schedule; the delay grows after each fire, up to about
  30 minutes.
- `timer.update` / `timer.pause` / `timer.resume` / `timer.stop` —
  maintain the loop. Pause before asking the human anything; stop when the
  objective is done.
- `timer.reset_delay` — you made real progress or new mail unblocked you:
  drop the delay back to the shortest interval. If you are still waiting
  with nothing to do, do NOT call it — end the turn.

## Human-facing and board tools (`codeg-mcp`)

- `check_user_feedback` — non-blocking snapshot of steering the human
  typed while you work. You never see it unless you call. Poll before
  starting implementation, before big decisions, and between sub-tasks;
  treat returned notes as high-priority steering. An empty result means
  carry on.
- `ask_user_question` — blocking multiple-choice cards, 1–4 questions,
  2–4 options each, `header` ≤ 12 chars. Only for decisions genuinely
  the user's to make; never for "should I proceed?" or open-ended
  questions. The user can always pick Other, so do not add your own. If
  dismissed, the result says so — proceed on your best judgment instead of
  re-asking.
- `get_session_info` — read-only lookup of a Session the human referenced
  (`session_id` from the `codeg://session/<id>` link). `max_messages`
  defaults to 20, 0 means metadata only, cap 200. `found: false` is a
  result, not an error.
- `create_automation` — save a repeating/scheduled job (5-field cron, IANA
  `timezone`; omit `cron` for manual). Each run is a fresh headless
  Session with no memory of this chat, so `prompt` must be self-contained.
  `action=enqueue_task` lands on the to-do board instead of running
  unattended. Creating does not run it now.
- `create_work_task` — park work as a card on the to-dos board for later
  execution. Not for work requested in this conversation.
- `task_progress` / `task_complete` — only in task-engine launches.
  Progress is fire-and-forget at real checkpoints; complete once at the
  end with `success` / `needs_review` / `blocked` and a summary. The task
  still settles at turn end if you never call it.

## @human discipline

The human is not a Room member and cannot be BCCed. Three channels reach
them, each with a distinct job:

- `mention_human=true` on `post_room` (or a `codeg://human` /
  `codeg://user` link in the body — both are detected) taps the operator's
  shoulder from a Room. It is recorded on the public post and wakes no
  Session. Use it when the run needs a human decision, a human-only
  capability (credentials, external accounts, physical state), or a
  verdict the team cannot legitimately make.
- `ask_user_question` is the blocking form for a discrete choice you
  cannot resolve from the request, the code, or defaults.
- `check_user_feedback` is the passive form — the human talks, you listen.

Everything else you digest on the Agent side: pick defaults, ask a peer by
mail or in the Room, record the decision on the ledger. Do not escalate to
ratify an obvious default, and do not `@human` as a broadcast —
`mention_all` deliberately excludes the human, so a human tap is always a
separate, intentional act.

Symmetric discipline for Sessions: `@` only who must act this round.
Structured mentions only — free-text `@name` wakes no one, and a quote
(`reply_to_event_id`) is not a wake.

## Delivery discipline

Settled conventions from the ledger; the pattern context is in
`patterns-map.md` and `lead-executor-split.md`.

- Done is delivered, or it is not done. A handoff is: write the
  deliverable to a file, then post in the Room (or letter) `@`ing whoever
  must act, carrying the path plus a one-line summary. The Room is the
  index; files carry the content. Never paste bulk code or full documents
  into messages.
- Every assignment letter goes out with `expects_reply=true` (the
  default). The obligation machine does the chasing: unread mail is nagged
  after about 5 minutes, an unanswered reply obligation after about
  5 minutes from receipt, at most 3 nags with a 5-minute cooldown — so
  workers who finish without reporting still surface. Audit your outbox
  with `list_inbox` `box=sent` `filter=awaiting_reply` instead of polling
  workers.
- Changed your mind mid-flight? Cancel first, then correct: Host Control
  `session.cancel_turn` on the worker, THEN the amended letter. A letter
  alone races a running turn that never checks its mailbox — the old
  instruction may already be executing.
- Parallel writers never share one working tree. One worktree or folder
  per writer; deliverables travel as paths in letters, never as code
  blocks.
- Reply chains cap at depth 4 — past the cap, `expects_reply` silently
  stops attaching. Each new round is a NEW root letter or post that quotes
  the previous round's event id or artifact path.

## Red lines

- Do not freeze a Session as planner/coder/reviewer — identity for a run
  is title + first prompt + the posts it sends, and a Session may take
  different jobs across rooms and days.
- Do not build persona packs, subscription filters, or role names as `@`
  addresses.
- `@` discipline stands: structured mentions only, `@` only who must act
  this round, no `mention_all` unless asked.
- Mailbox is BCC. There is no CC, and recipients cannot see each other —
  do not simulate one by quoting one recipient's letter to another.
