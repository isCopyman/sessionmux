# Pattern template

Copy this shape for a this-run plan or a saved pattern. Keep it short.
Fill every heading. A Session is a participant for this run, not a job title
that outlives the run.

```markdown
# <pattern-slug>

## Scenario
When to use this. One or two sentences. What this is not.

## Participants
- Session address (`codeg://session/<id>` once known) or “create harness X”
- This-run title (display only; not an @ address)
- This-run job in `initial_prompt` / first Room post / mailbox body
The same Session may hold a different job next time.

## Channels
Order of operations. What belongs on the Room ledger. What is mailbox-only.

## Wake
Who gets `mention_session_ids` / `mention_all` / `mention_human` on which
posts. Quote-reply does not wake. Do not @ people who only need to read
later via `read_room`.

## Stop
When the run is done. Who posts the closing note. Who must not keep pinging.
```

Do not add: system prompt, persona pack, subscription filters, squad owner,
or a second addressing namespace besides Session id.
