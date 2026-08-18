# Persist and sync

Codeg keeps **one** skill tree in `~/.codeg/skills/<id>/`. Each harness
gets a link (symlink / Windows junction) into that tree. That is the sync
mechanism. Do not copy playbooks into `.claude/skills`, `.codex/skills`,
or other harness trees.

Multica collaboration is a **CLI** (`multica …`, `allowed-tools: Bash(multica *)`)
plus a workspace DB that materializes files per run. Codeg collaboration
is **MCP** (mailbox, room, Host Control). Do not shell out to a Codeg CLI
to send mail or post. Do not write a second copy of this playbook “for Codex”.

## What is allowed to change

| Location | Who writes | Survives Codeg upgrade? |
|---|---|---|
| This bundled skill `codeg-multi-agent/` | Product | Yes, but **re-extracted** — agent writes here are wiped or backed up |
| Project `.codeg/collab-patterns/<slug>.md` | Agent after human confirms | Yes, it is project files |
| Custom skill `~/.codeg/skills/codeg-collab-patterns/` | Agent after human confirms | Yes — custom ids are not bundled |
| Harness-private skill dir (copy, not a link) | Nobody | No, and other harnesses never see it |

On Windows, if a skill enable fell back to a **copy** instead of a junction,
do not write through that copy. Write to the central path above and tell
the human the link is copy-mode.

## After a run worth repeating

1. Draft a file from `pattern-template.md`. Use a slug like
   `auth-review-roundtable.md`.
2. Show the draft. Do not save yet.
3. If the human confirms **project-level**: create
   `<workspace>/.codeg/collab-patterns/` if needed and write the file there.
4. If the human confirms **user-global**:
   - If `~/.codeg/skills/codeg-collab-patterns/SKILL.md` is missing, create
     that custom skill with a short SKILL.md that says: load
     `references/*.md` when picking a collaboration pattern.
   - Write the pattern under
     `~/.codeg/skills/codeg-collab-patterns/references/`.
   - Tell the human to enable `codeg-collab-patterns` in Codeg Experts for
     every harness that should see it. Enablement is per harness; content
     is shared.
5. Never write into `codeg-multi-agent/` (this bundled skill).
6. Never treat one successful run as automatically canonical. A draft that
   froze an accident stays a draft.

## Loading order

1. Product files in this skill's `references/` (except this file and the
   template).
2. `<workspace>/.codeg/collab-patterns/*.md` if that directory exists.
3. `~/.codeg/skills/codeg-collab-patterns/references/*.md` if present.

Prefer a project file over a user-global file with the same slug.
