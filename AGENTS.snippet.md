## Relatum: shared notes for spawned agents

This project uses Relatum (`.swarmnotes/`) to share context between agents
by reference. A note is a short markdown file; its ID is its path under
`.swarmnotes/` without `.md` (e.g. `net/retry-decision`).
The storage directory stays `.swarmnotes/`.

**When you spawn agents (parent):**
- Anything more than one child needs (decisions, constraints, conventions,
  context, report format) goes in a note *once*. Don't paste it into briefs.
- Write each brief as:
  ```
  Task: <what to do, 1-2 sentences>
  Read first: <note IDs the child must read>
  Your scope: <directory under .swarmnotes/ the child writes to>
  Done when: <checkable end condition>
  ```
- Always include `_shared/report-format` so reports come back short.
- Changed your mind? Make a small fix by editing the note in place. If the
  note is being replaced, write a new one and supersede the old id.

**When you are spawned (child):**
- Read every note in "Read first" before doing anything else.
- You may list your own scope and `_shared` for sibling findings and failed
  attempts. Don't browse other scopes unless told to.
- Picking work up later? List with `details` and keep the newest
  `modified`; next time pass it as `since` to see only what changed.
- Record what you learn as `finding` notes and what failed as `attempt` notes
  in your scope. In your report, cite their IDs instead of restating them.
- Only edit notes you wrote. If you disagree with one, write a `finding` and
  flag it in your report.

**Prefer the MCP tools** when this host exposes them. They use the same
`.swarmnotes/` files as the CLI:

```
relatum_ls        scope, kind, tag, include_superseded; limit, since, details
relatum_get       one or more IDs; follows superseded notes
relatum_new       id, kind, summary, body, author, tags
relatum_supersede old, new
relatum_check
relatum_init      create .swarmnotes/ if it is missing (cwd or SWARMNOTES_DIR only)
```

Otherwise use the CLI (`relatum`; `swarmnotes` is the same command):

```
relatum ls [scope] [--kind K] [--tag T] [--all] [--limit N] [--since TIME] [--details]
relatum get ID [ID...]
relatum new ID --kind K --summary "..." --author NAME --body "..."
relatum supersede OLD NEW
relatum check
```

Kinds: decision, constraint, convention, context, format (parent);
finding, attempt (child). Keep a summary to one line (<= 100 chars) and a body
under about 300 words. If it is longer, split it.

Without MCP or the CLI: `ls -R .swarmnotes/` is the catalog and
`cat .swarmnotes/<id>.md` is get. To create a note, write a new file with
frontmatter (`kind`, `summary`, `author`, `updated`), but never overwrite an
existing one.
