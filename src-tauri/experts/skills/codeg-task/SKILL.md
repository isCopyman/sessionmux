---
name: codeg-task
description: Use when a managed Codeg Agent should capture, inspect, refine, claim, assign, progress, or complete cards on Codeg's task board. Use codeg-host-control for Session/Collection/Workbench management, codeg-room for shared discussion, and codeg-mailbox for private messages.
---

# Codeg Task Board

Use the task board as the durable statement of what work exists and its current
business state. A task is not a Session, Room, chat transcript, or project file.

## Capture and inspect

- `create_work_task` creates an unassigned card. Use `initial_status=backlog`
  for an uncommitted idea; omit it for a ready Todo. Creating a card never
  starts an Agent, chooses a Harness/Profile/model, or creates a worktree.
- Use `list_tasks` to avoid duplicates and `get_task` before changing or
  accepting a card. The default scope is the caller's project; request the
  global scope only when the work genuinely spans projects.
- `task.update` through `codeg_help` / `codeg_use` refines the title or
  description of an unassigned card or the caller's own active card. It does
  not rewrite a Prompt that already entered a Harness.

## Choose execution explicitly

- `task.claim` atomically assigns an unowned card to the token-derived current
  Session, moves it to In Progress, and queues its brief as the next normal
  Prompt. Never pass the caller's Session id.
- `task.assign` does the same for another persistent Session using its stable
  `target_session_id`. Discover the schema and target ID before writing.
- To delegate a card to a new worker, use the existing Host Control primitives:
  call `session.create` without `initial_prompt`, keep its returned stable
  Session ID, then call `task.assign` with that ID. `task.assign` supplies the
  authoritative task brief, so duplicating it as `initial_prompt` would start
  the same work twice. A failed assignment does not turn the new Session into a
  hidden task runtime; it remains an ordinary persistent Session that can be
  inspected or assigned again.
- A competing claim is rejected rather than stealing the task. Moving a card
  between board columns is organization only and never starts or wakes an
  Agent.

## Report work

- Call `task_progress` for meaningful milestones, decisions, blockers, or a
  stable path to a deliverable—not for every small action.
- Call `task_complete` only when the whole card is ready for human review, or
  when it is blocked. Finishing a single Turn does not finish the task.
- The seven fixed business states are `backlog`, `todo`, `in_progress`,
  `review`, `done`, `blocked`, and `canceled`. Agent completion normally moves
  to Review or Blocked; the human accepts Done, cancels, or reopens work.

Keep long-lived plans, evidence, and mutable deliverables in project files.
Keep the task description and progress timeline concise, linking stable paths
instead of copying large artifacts into the database. Use a Session for the
assignee's full execution context and a Room when several Sessions need a
shared discussion timeline.
