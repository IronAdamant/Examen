# Examen

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Python 3.9+](https://img.shields.io/badge/python-3.9%2B-blue.svg)](https://www.python.org/downloads/)

**Pass-by-reference notes for agent swarms.**

When an AI agent spawns subagents, it usually passes context by value: it
pastes the same decisions, constraints, and conventions into every spawn
prompt. That text is paid for in the parent's output tokens, it sits in the
parent's context window, and it gets re-explained again and again.

Examen, formerly SwarmNotes, lets the parent write each fact **once** as a
short note and send children a brief that names note IDs:

```
Task: add retry logic to src/acme/net/client.py per the retry decision.
Read first: _shared/report-format, _shared/repo-conventions, net/retry-decision, net/no-new-deps
Your scope: net/client
Done when: client retries per the decision; existing tests still pass.
```

Children fetch what they're told to, write findings and dead ends back as
notes, and return short reports that cite note IDs. Siblings can see each
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
Cursor, or anything else. The directory stays `.swarmnotes/` so existing paths
keep working.

- **[SPEC.md](SPEC.md)** — the format, the parent/child protocol, and lint
- **[examples/retry-swarm](examples/retry-swarm/WALKTHROUGH.md)** — a parent and three children, end to end
- **[AGENTS.snippet.md](AGENTS.snippet.md)** — paste into a project's `AGENTS.md` / `CLAUDE.md`
- **[skills/examen](skills/examen/SKILL.md)** — the same instructions as an agent skill
- **[templates/](templates/)** — blank brief and note

## Install

The CLI has **no third-party dependencies**. `pip install examen` installs
the command and nothing else. The MCP server is an optional extra and is the
only install that depends on the official MCP Python SDK.

```sh
pip install examen                 # CLI, zero dependencies (Python >= 3.9)
pip install 'examen[mcp]'          # CLI + stdio MCP server (Python >= 3.10)
```

From a checkout, `python3 examen.py` is the same CLI. `uv tool install examen`
works too. `swarmnotes` on `PATH` is an alias of `examen`.

## CLI

```sh
examen init                                   # .swarmnotes/ + default report format
examen new net/retry-decision --kind decision \
  --summary "Exponential backoff, max 4 attempts" --author parent --body "..."
examen ls net --kind decision                 # id, kind, summary
examen get net/retry-decision _shared/report-format
examen supersede net/backoff-linear net/retry-decision
examen check                                  # lint
```

Set `SWARMNOTES_DIR` to point at a store without walking up from the working
directory. `ls` hides superseded notes unless `--all`. `get` follows
`superseded_by`, including multi-hop chains.

## MCP server

`examen-mcp` is a stdio MCP server over **the same files** as the CLI. There
is no second store and no hosted service. Hosts that speak MCP — Claude Code,
Claude Desktop, Cursor, Grok Bot connectors, and other MCP clients, including
Chinese IDEs — launch this one command.

Tools: `examen_init`, `examen_ls` (scope, kind, include_superseded),
`examen_get` (one or more IDs; follows superseded notes), `examen_new`,
`examen_supersede`, `examen_check`.

The server finds `.swarmnotes/` the same way the CLI does (walk up from the
working directory, or `$SWARMNOTES_DIR`). Set `SWARMNOTES_DIR` to the absolute
store path when the host does not start the process inside the project.
`examen init` / `examen_init` creates the directory.

If `examen-mcp` is not on `PATH`, use `python3 -m examen_mcp` with the
interpreter that has `examen[mcp]` installed.

### Claude Code and Claude Desktop

Project `.mcp.json` (Claude Code) or `claude_desktop_config.json`
(Claude Desktop):

```json
{
  "mcpServers": {
    "examen": {
      "type": "stdio",
      "command": "examen-mcp",
      "args": [],
      "env": {
        "SWARMNOTES_DIR": "/absolute/path/to/project/.swarmnotes"
      }
    }
  }
}
```

Claude Code can also add it from the shell: `claude mcp add examen -- examen-mcp`.

### Cursor and any other MCP client

`.cursor/mcp.json` in the project, or `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "examen": {
      "command": "examen-mcp",
      "args": [],
      "env": {
        "SWARMNOTES_DIR": "/absolute/path/to/project/.swarmnotes"
      }
    }
  }
}
```

The same JSON, with a stdio `command`, is what other MCP hosts ask for. Point
them at `examen-mcp`. Prefer the MCP tools when the host exposes them;
otherwise use the CLI or the files. See [AGENTS.snippet.md](AGENTS.snippet.md).

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

v0.3.0. The note format is v0.2; this release adds the MCP server. See
[CHANGELOG.md](CHANGELOG.md).

## Tests

```sh
python3 -m unittest discover -s tests
cd examples/retry-swarm && python3 ../../examen.py check
```

## GitHub topics

Topics are set in the repository settings, not from this tree. Suggested:
`ai-agents`, `mcp`, `model-context-protocol`, `llm`, `claude`, `cursor`,
`python`, `agent-tools`.

## License

MIT. See [LICENSE](LICENSE). Contributing notes are in [CONTRIBUTING.md](CONTRIBUTING.md).
