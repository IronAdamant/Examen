# Relatum format - v0.2 notes

Relatum is pass-by-reference for agent context. A parent agent writes a fact
once as a **note**, then hands spawned agents a short **brief** that names note
IDs instead of pasting their text. Children fetch only the notes they are told
to (or that they find relevant in their own scope), and write their findings
back as notes so siblings and the parent can reference them too.

The format is plain markdown files. Any agent that can read and write files can
participate; the `relatum` CLI and the MCP server are conveniences over
the same files. `swarmnotes` is an alias of the same CLI. The store directory
is still `.swarmnotes/`.

## 1. Layout

```
.swarmnotes/
  _shared/                  # swarm-wide notes (conventions, report format)
    report-format.md
    repo-conventions.md
  net/                      # a scope: one subtree of the swarm's work
    retry-decision.md
    no-new-deps.md
    client/                 # scopes nest freely
      timeout-bug.md
```

- The root is the nearest `.swarmnotes/` directory walking up from the working
  directory, or `$SWARMNOTES_DIR` if set.
- The storage directory stays `.swarmnotes/` so existing paths keep working.
  v0.3 does not rename it.
- A **scope** is any directory under the root. Give each branch of a swarm its
  own scope so listings stay small no matter how big the swarm gets.
- `_shared` is reserved for notes every agent in the swarm may need.

## 2. Note IDs

A note's ID is its path relative to the root, without `.md`:

```
.swarmnotes/net/retry-decision.md   ->   net/retry-decision
```

- Segments match `_?[a-z0-9][a-z0-9-]*`, joined by `/`.
- The slug should say what the note is about. `net/retry-decision` in a brief
  is already informative before anyone fetches it.
- IDs are created exclusively: if the file exists, pick another name. Never
  overwrite a note to "create" it.
- The CLI and the MCP server publish each write whole: they write a temp
  file beside the note (its name does not end in `.md`), then hard-link it
  into place for a new note or rename it over the note for `supersede`. A
  concurrent reader sees the old note or the new one, never a partial file.

## 3. Note file

```markdown
---
kind: decision
summary: Retries use exponential backoff with jitter, max 4 attempts.
author: parent
updated: 2026-10-03
tags: [net, retry]
---

Use exponential backoff (base 200ms, factor 2, full jitter), max 4 attempts.
Retry only on connection errors and 502/503/504. Never retry POST unless the
request carries an idempotency key.

Why: the upstream rate-limits bursts; linear backoff caused thundering herds.
```

Frontmatter is a flat subset of YAML: `key: value` lines, with lists written
inline as `[a, b]`. A `#` comment line is allowed. Fields:

| field           | required | meaning                                                         |
|-----------------|----------|-----------------------------------------------------------------|
| `kind`          | yes      | one of the kinds below                                          |
| `summary`       | yes      | one line, <= 100 chars; shown in listings                        |
| `author`        | no       | who wrote it (`parent`, or the agent's name)                    |
| `updated`       | no       | ISO date (`YYYY-MM-DD`) of last edit                            |
| `tags`          | no       | inline list of slugs, for filtering                             |
| `status`        | no       | `active` (default), `draft`, or `superseded`                    |
| `superseded_by` | no       | ID of the replacing note; required when `status: superseded`    |

Unknown keys, duplicate keys, and a `superseded_by` on a note that is not
`superseded` are lint errors. Tag slugs match `[a-z0-9][a-z0-9-]*`.

### Kinds

| kind         | written by | holds                                                       |
|--------------|------------|-------------------------------------------------------------|
| `decision`   | parent     | a choice already made, and why - so children don't relitigate it |
| `constraint` | parent     | something that must / must not be done                      |
| `convention` | parent     | how things are done here (style, layout, commands)          |
| `context`    | parent     | background a child needs but can't cheaply rediscover       |
| `format`     | parent     | how output should be shaped (e.g. the report format)        |
| `finding`    | child      | something learned while working                             |
| `attempt`    | child      | an approach tried that failed, and why                      |

### Size

A note is a fact, not a document. Keep the body under ~300 words. If it grows
past that, split it into several notes. A note that's too big just moves the
token waste from the brief into the fetch. `relatum check` warns past 300 words
and does not fail the command for length alone.

## 4. Briefs

A brief is the spawn prompt: per-child instructions that **reference** notes.
It is not stored; it's whatever the parent passes when spawning.

```
Task: add retry logic to the HTTP client in src/net/client.py.
Read first: _shared/report-format, _shared/repo-conventions, net/retry-decision, net/no-new-deps
Your scope: net/client  (write findings/attempts here)
Done when: tests/net/ passes and retries are covered by a new test.
```

Rules for a brief:

1. **Task** - what to do, in a sentence or two. This is the only part that's
   unique to the child.
2. **Read first** - the exact note IDs the child must read. The parent decides
   relevance; children don't have to guess.
3. **Your scope** - where the child writes notes, and the subtree it may
   browse for sibling findings.
4. **Done when** - a checkable end condition.

Never paste a note's body into a brief. If the text is worth sending to more
than one child, it's worth making it a note.

## 5. Protocol

### Parent

- Write a note for anything more than one child needs, or anything you'll
  need to restate later.
- Name the required note IDs in every brief.
- Give each child (or group of children) a scope.
- Changed your mind? Make a small correction by editing the note in place and
  bumping `updated` (`relatum_edit` over MCP). If the note is being replaced, write a new note and mark
  the old one `status: superseded` with `superseded_by` (`relatum supersede OLD NEW`).
- Read children's reports. Fetch the note IDs they cite only when you need the
  details.

### Child

- Read every note named in **Read first** before starting.
- You may list your own scope (and `_shared`) to find sibling findings and
  failed attempts. Don't browse other scopes unless the brief says to.
- Write what you learn as `finding` notes and what failed as `attempt` notes
  in your scope. Your final report cites their IDs instead of restating them.
- Only edit notes you authored. If you disagree with a parent note, write a
  `finding` and say so in your report.
- If a note you were given is superseded, follow `superseded_by`. `relatum get`
  follows the chain, including multiple hops, and stops on a note that is not
  superseded.

## 6. Lifecycle

Whether `.swarmnotes/` is committed is up to the project. A common split:
commit `_shared/` (durable conventions), gitignore task scopes (ephemeral
swarm state), and delete a scope when its work is merged.

## 7. Lint

`relatum check` reads every `*.md` note under the root.

Errors (exit 1). `ls` still hides superseded notes unless `--all`, and this
list is what v0.2 treats as a hard failure:

- id segments that do not match the id grammar, or a file that is not UTF-8
- missing or unclosed frontmatter; a frontmatter line that is not `key: value`
- duplicate or unknown frontmatter keys
- `kind` missing or not one of the kinds above
- `summary` missing or longer than 100 characters
- empty body
- `status` not `active`, `draft`, or `superseded`
- `superseded` without `superseded_by`, a target that is not a valid id, a
  missing target, a cycle, a chain that does not end on a live note, or a
  chain longer than 32 hops
- `superseded_by` set when `status` is not `superseded`
- `tags` that are not an inline list of slugs
- `updated` that is not a real `YYYY-MM-DD` date
- empty `author` when the key is present

Warnings (printed, exit 0 when there are no errors):

- body over 300 words
- missing `author`
- missing `updated`

`relatum ls` hides `superseded` notes unless `--all`. With `--all`, a
superseded row is marked `[-> <id>]`. Drafts are marked `[draft]`.
Opt-in, and absent from the default output: `--limit N` prints at most N
rows and then `... K more (...)`; `--since TIME` keeps notes whose file
was modified at or after TIME, a UTC `YYYY-MM-DD` or
`YYYY-MM-DDTHH:MM:SSZ` (every write sets it, so this is "written or
changed since"); `--details` appends `by <author>` and `at <modified>`.
The note format has no creation time; `updated` is the date of the last
edit. `relatum_ls` takes the same `limit`, `since`, and `details`; it
reports left-out rows as `more` and adds `author`, `updated`, and
`modified` per note.
`relatum get` prints the live note at the end of the chain and the path it
followed.

## 8. Versioning

Relatum 1.0.0 keeps the v0.2 note format. The CLI and the MCP server are a
Rust program with no Cargo dependencies. It is not published to crates.io
or PyPI. The names `examen` on those indexes belong to unrelated projects.
npm packages are `@iron_adamant/relatum` and `@iron_adamant/relatum-<os>-<arch>`.

Tools: `relatum_init`, `relatum_ls`, `relatum_get`, `relatum_new`,
`relatum_edit`, `relatum_supersede`, `relatum_check`. `relatum_edit` is the
in-place correction of section 5: it replaces a live note's summary, body,
or tags and sets `updated` to today, and refuses unless `author` matches
the note's author (a note with no author takes the editor's). A superseded
note cannot be edited; replacing a note is a new note plus
`relatum_supersede`, and the errors say so. `relatum_init` only creates a store in
the current working directory or at `SWARMNOTES_DIR`. A tool result is a
JSON object in `structuredContent.result`, repeated as compact JSON text.
A tool error names the next step: with no store, call `relatum_init` or set
`SWARMNOTES_DIR`; for a scope with no notes yet, write one with
`relatum_new`; for an invalid id, the id grammar above. An argument of the
wrong type, or a missing one, is a one-line error starting `invalid
arguments: ` that names the field, the value received when short, what
was expected, and how to call again.

`.swarmnotes/` and `$SWARMNOTES_DIR` stay, so a store written by Examen 0.3.0
or by the earlier SwarmNotes files loads with no migration. v0.2 made
structural frontmatter, tags, dates, empty bodies, and supersede chains into
lint errors, and `get` / `supersede` refuse a chain that does not end on a
live note. 1.0.0 treats that format as stable.
