---
name: examen
description: Share context with spawned subagents by reference using Examen (.swarmnotes/ note files). Use when delegating work to subagents, writing briefs or spawn prompts for more than one agent, or when a brief names Examen note IDs to read first.
---

## Examen: shared notes for spawned agents

This project uses Examen (`.swarmnotes/`) to share context between agents
by reference. A note is a short markdown file; its ID is its path under
`.swarmnotes/` without `.md` (e.g. `net/retry-decision`).
The storage directory stays `.swarmnotes/` in v0.2.

**When you spawn agents (parent):**
- Anything more than one child needs (decisions, constraints, conventions,
  context, report format) goes in a note *once*. Don't paste it into briefs.
- Write each brief as:
  ```
  Task: <what to do, 1–2 sentences>
  Read first: <note IDs the child must read>
  Your scope: <directory under .swarmnotes/ the child writes to>
  Done when: <checkable end condition>
  ```
- Always include `_shared/report-format` so reports come back short.
- Changed your mind? Make a small fix by editing the note in place. If the
  note is being replaced, write a new one and run `examen supersede OLD NEW`.

**When you are spawned (child):**
- Read every note in "Read first" before doing anything else.
- You may list your own scope and `_shared` for sibling findings and failed
  attempts. Don't browse other scopes unless told to.
- Record what you learn as `finding` notes and what failed as `attempt` notes
  in your scope. In your report, cite their IDs instead of restating them.
- Only edit notes you wrote. If you disagree with one, write a `finding` and
  flag it in your report.

**Commands** (`examen` CLI; `swarmnotes` is the same command; plain file reads and writes work too):
```
examen ls [scope] [--kind K] [--tag T] [--all]
examen get ID [ID...]                 # bodies; follows superseded notes
examen new ID --kind K --summary "…" --author NAME --body "…"
examen supersede OLD NEW
examen check                          # lint
```
Kinds: decision, constraint, convention, context, format (parent);
finding, attempt (child). Keep a summary to one line (≤ 100 chars) and a body
under ~300 words. If it's longer, split it.

Without the CLI: `ls -R .swarmnotes/` is the catalog and `cat .swarmnotes/<id>.md`
is get. To create a note, write a new file with frontmatter (`kind`, `summary`,
`author`, `updated`), but never overwrite an existing one.
