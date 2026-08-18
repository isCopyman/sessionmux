---
name: codeg-session-timer
description: Keep one managed Codeg Session advancing through a long objective after each completed Turn. Use when the user asks the current Session to keep working, finish a multi-step task, or continue from a durable project plan without needing another user prompt. Also use to inspect, update, pause, resume, reset the reminder delay, or stop the current Session's continuation timer.
---

# Codeg Session Timer

Use Codeg's progressive Host Control MCP. Do not use shell timers, a CLI,
AgentBus, another Agent, or a model-supplied Session id.

## Start a continuation loop

1. Call `codeg_help` for `timer.list`, then `codeg_use` to check the current
   Session's timers. Reuse or update an existing timer instead of stacking
   overlapping loops.
2. For a long task, keep the detailed objective, decisions, completed work,
   blockers, and next step in a project file such as `docs/current-task.md`.
3. Call `codeg_help` for `timer.create`, then create a short continuation
   prompt. Include the plan-file path when one exists.

Prefer a prompt shaped like:

```text
Continue the current objective. Read docs/current-task.md as the source of
truth, perform the next unfinished step, and update the file with progress.
When the objective is complete, stop this timer before your final response.
If blocked on user input, pause the timer before asking.
```

The backend waits for a real ACP `TurnComplete` boundary and enqueues this text
as the next ordinary user follow-up. It does not start a judging sub-Agent and
does not create a separate Goal state machine.

## Maintain the loop

- Update the project file after meaningful progress so the next Turn resumes
  from durable facts rather than reconstructing the whole conversation.
- When you made real progress, or new mail / Room news unblocks you, call
  `timer.reset_delay`. That returns the reminder delay to the shortest
  interval (`idle_grace_seconds`). The wait is a **delay**, not a clock
  schedule: after each fire it doubles, up to about 30 minutes, until you
  reset it.
- If you are still waiting on someone else and have nothing else to do,
  do **not** call `timer.reset_delay`. End the turn. Do not take over
  another Session's work just to keep the delay short.
- Mail, Room `@`, and the human typing still wake you through the
  Dispatcher. After that wake, if you can continue the goal, work, then
  `timer.reset_delay`.
- When the objective or next-step policy changes, update the project file and
  call `timer.update` with revised continuation text.
- Call `timer.pause` before asking the user for information, approval, or an
  external state change. Call `timer.resume` after the blocker clears.
- Call `timer.stop` before the final response when the objective is complete.
  The same Agent doing the work makes this decision.

One Session, one active continuation. Change the timer text or the plan file
instead of stacking timers.

Always discover the current schema with `codeg_help` before `codeg_use`.
Codeg derives the current Session from the managed MCP token; never include a
caller, `from`, current Session id, or request id.
