# Examen

**Pass-by-reference notes for agent swarms.**

When an AI agent spawns subagents, it usually passes context by value: it
pastes the same decisions, constraints and conventions into every spawn prompt.
That's paid for in the parent's output tokens, it sits in the parent's context
window, and it gets re-explained again and again.

Examen, formerly SwarmNotes, lets the parent write each fact **once** as a
short note and send children a brief that names note IDs:

```
Task: add retry logic to src/acme/net/client.py per the retry decision.
Read first: _shared/report-format, _shared/repo-conventions, net/retry-decision, net/no-new-deps
Your scope: net/client
Done when: client retries per the decision; existing tests still pass.
```

Children fetch what they're told to, write their findings and dead ends back
as notes, and return short reports that cite note IDs. Siblings can see each
other's findings, so nobody repeats a failed approach.

## It's just files

```
.swarmnotes/
  _shared/report-format.md
  net/retry-decision.md          <- id: net/retry-decision
  net/client/timeout-bug.md      <- written back by a child
```

A note is markdown with a little frontmatter (`kind`, `summary`, …). Its ID is
its path. Any agent that can read files can take part, in Claude Code, Grok,
Cursor, or anything else. No server is required.

The storage directory stays `.swarmnotes/` in v0.2 so existing dogfood paths keep working.

- **[SPEC.md](SPEC.md)**: the format, the parent/child protocol, and the v0.2 lint
- **[examples/retry-swarm](examples/retry-swarm/WALKTHROUGH.md)**: a parent and three children, end to end
- **[AGENTS.snippet.md](AGENTS.snippet.md)**: paste into a project's `AGENTS.md` / `CLAUDE.md` to teach agents the convention
- **[skills/examen](skills/examen/SKILL.md)**: the same instructions as an agent skill
- **[templates/](templates/)**: blank brief and note

## CLI

Stdlib only. Nothing to vendor.

```sh
pip install .            # or: uv tool install .   or run python3 examen.py directly

examen init                                   # .swarmnotes/ + default report format
examen new net/retry-decision --kind decision \
  --summary "Exponential backoff, max 4 attempts" --author parent --body "..."
examen ls net --kind decision                 # id, kind, summary
examen get net/retry-decision _shared/report-format
examen supersede net/backoff-linear net/retry-decision
examen check                                  # lint
```

The `swarmnotes` console script runs the same CLI. Set `SWARMNOTES_DIR` to
point at a store without walking up from the working directory. `ls` hides
superseded notes unless `--all`. `get` follows `superseded_by`, including
multi-hop chains.

## Design principles

- **The parent decides what's relevant.** Briefs name note IDs; children
  don't have to guess from a catalog.
- **Scopes keep listings small.** Each branch of a swarm gets its own
  directory, so `ls` stays cheap at thousands of notes.
- **Notes are facts, not documents.** One-line summary, body under ~300 words.
- **Nothing silently goes stale.** Replaced notes are marked superseded, and
  `get` follows the link.
- **Write-back is part of the protocol.** Children's findings and failed
  attempts are notes too.

## Status

v0.2.0. Lint covers ids, frontmatter, kinds, summaries, tags, dates, empty
bodies, and supersede chains. An MCP server is not part of this release.

Next:

- Dogfood on real fan-out tasks and measure parent output tokens, total
  tokens, and child mistakes caused by missing context, with and without notes.
- MCP server over the same files, for hosts where that's more natural than a CLI.
- Adapters and snippets for more hosts.

## Tests

```sh
python3 -m unittest discover -s tests
cd examples/retry-swarm && python3 ../../examen.py check
```

## License

MIT. See [LICENSE](LICENSE).
