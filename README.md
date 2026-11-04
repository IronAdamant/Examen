# Relatum

Formerly Examen.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Pass-by-reference notes for agent swarms.**

When an AI agent spawns subagents, it usually passes context by value: it
pastes the same decisions, constraints, and conventions into every spawn
prompt. That text is paid for in the parent's output tokens, it sits in the
parent's context window, and it gets re-explained again and again.

Relatum lets the parent write each fact **once** as a short note and send
children a brief that names note IDs:

```
Task: add retry logic to src/acme/net/client.py per the retry decision.
Read first: _shared/report-format, _shared/repo-conventions, net/retry-decision, net/no-new-deps
Your scope: net/client
Done when: client retries per the decision; existing tests still pass.
```

Children fetch what they are told to, write findings and dead ends back as
notes, and return short reports that cite note IDs. Siblings can see each
other's findings, so nobody repeats a failed approach.

## It is just files

```
.swarmnotes/
  _shared/report-format.md
  net/retry-decision.md          <- id: net/retry-decision
  net/client/timeout-bug.md      <- written back by a child
```

A note is markdown with a little frontmatter (`kind`, `summary`, ...). Its ID
is its path. Any agent that can read files can take part, in Claude Code,
Grok Build, Cursor, or anything else. The directory stays `.swarmnotes/` so
stores written by Examen 0.3.0, and by the earlier SwarmNotes files, load
with no migration and no second copy. `SWARMNOTES_DIR` still points at a
store. `relatum init` writes the same seeded `_shared/report-format` note as
Examen 0.3.0, including `author: examen`, so a store created on the same day
matches byte for byte.

- **[SPEC.md](SPEC.md)**: the format, the parent/child protocol, and lint
- **[examples/retry-swarm](examples/retry-swarm/WALKTHROUGH.md)**: a parent and three children, end to end
- **[AGENTS.snippet.md](AGENTS.snippet.md)**: paste into a project's `AGENTS.md` / `CLAUDE.md`
- **[skills/relatum](skills/relatum/SKILL.md)**: the same instructions as an agent skill
- **[templates/](templates/)**: blank brief and note

## Install

The program is Rust, standard library only. There are no Cargo crates.
It is not published to crates.io or PyPI. The name `examen` on PyPI and on
the unscoped npm package belongs to unrelated projects. Relatum's packages
are scoped `@iron_adamant`.

### npm

```sh
npx -y @iron_adamant/relatum --help
npx -y @iron_adamant/relatum mcp
```

`npx` installs `@iron_adamant/relatum` and the optional platform package for
this OS. The launcher is JavaScript with no npm dependencies. It runs
`relatum`, `relatum-mcp`, or `swarmnotes` from that platform package.
Platform packages:

| Package | OS | Arch |
|---|---|---|
| `@iron_adamant/relatum-linux-x64` | Linux | x64 |
| `@iron_adamant/relatum-linux-arm64` | Linux | arm64 |
| `@iron_adamant/relatum-darwin-x64` | macOS | x64 |
| `@iron_adamant/relatum-darwin-arm64` | macOS | arm64 |
| `@iron_adamant/relatum-windows-x64` | Windows | x64 |

### GitHub release binaries

A `v*` tag builds linux x64, linux arm64, macOS x64, macOS arm64, and
Windows x64 archives and attaches them to the GitHub Release with
`SHA256SUMS`. Verify the checksums before installing. Each archive contains
`relatum`, `relatum-mcp`, and the `swarmnotes` alias. macOS archives are
named `relatum-macos-x64` and `relatum-macos-arm64`. The matching npm
packages use `darwin` in the name, which is the id Node uses for macOS.
This tree does not publish a tag by itself.

<https://github.com/IronAdamant/Relatum/releases>

### From source

```sh
git clone https://github.com/IronAdamant/Relatum
cd Relatum
cargo build --release --locked
```

The binaries are `target/release/relatum` and `target/release/relatum-mcp`.
`target/release/swarmnotes` is the same CLI under the old alias. Copy them
onto `PATH` if you want the short names. Rust 1.83 or newer is enough.

## Migrating from Examen (Python)

Examen 0.3.0 is the `python-legacy` branch. Relatum 1.0.1 reads the same
`.swarmnotes/` files. `parity/harness.py` runs both and rewrites the items
below before it treats them as equal. Each item is an intentional change
or is unchanged.

- **Intentional change.** The CLI is `relatum`. The stdio server is
  `relatum-mcp`, and `relatum mcp` starts that same server. Examen had no
  `mcp` subcommand. It used `examen-mcp` or `python -m examen_mcp`.
  `relatum --help` still lists the same six commands as Examen
  (`init`, `new`, `ls`, `get`, `supersede`, `check`). `relatum mcp -h`
  prints the server command's help.
- **Intentional change.** MCP tools are `relatum_init`, `relatum_ls`,
  `relatum_get`, `relatum_new`, `relatum_supersede`, and `relatum_check`.
  Examen used `examen_*`. The server instructions say Relatum, and
  `serverInfo.name` is `relatum`. The harness rewrites `relatum_` to
  `examen_`, then `Relatum` to `Examen`, then `relatum` to `examen`, before
  comparing MCP text. For the CLI comparison it also forces Examen's
  program name to `relatum` or `swarmnotes`, so help and errors can be
  compared after the rename.
- **Unchanged.** `swarmnotes` is still an alias of the CLI. Help and errors
  use that name when the binary is `swarmnotes`.
- **Intentional change.** Install is no longer the Examen command
  `pip install "examen[mcp] @ git+https://github.com/IronAdamant/Examen"`.
  Relatum is this repository. Use npm, a GitHub Release archive, or
  `cargo build --release --locked`. The MCP server is in the binary. There
  is no Python extra to install, and the process does not exit asking for
  one.
- **Unchanged.** The store directory is `.swarmnotes/`. `SWARMNOTES_DIR`
  still points at a store. Notes written by Examen 0.3.0 load with no
  migration and no second copy.
- **Unchanged.** `init` still writes `author: examen` on
  `_shared/report-format`. The rest of that seeded note matches Examen
  0.3.0, so a store created on the same local date matches byte for byte.
  The harness compares those file bytes as-is and does not rewrite
  `author`.
- **Unchanged.** On Windows, Relatum writes CRLF for CLI stdout, CLI
  stderr, and MCP stderr log lines. The MCP logger does that so the log
  bytes match CPython's text-mode stderr. The harness folds CR and LF on
  the captured CLI pipes and on MCP stderr, then compares the line text.
  MCP JSON-RPC responses are written with LF. The harness parses those
  responses as JSON and does not rewrite that stream.
- **Intentional change.** MCP `serverInfo.version` is the Relatum package
  version, 1.0.1. Examen reported 0.3.0. The harness rewrites `1.0.1` and
  `1.0.0` to `0.3.0` before comparing.
- **Intentional change.** Validation errors keep pydantic's wording. The
  documentation URL is pinned to `https://errors.pydantic.dev/2.14/v/...`
  in `src/mcp_support.rs`. Examen printed the version of whichever pydantic
  was installed with the MCP extra. The harness rewrites
  `https://errors.pydantic.dev/<version>/` to
  `https://errors.pydantic.dev/VER/` on both sides, then compares the
  validation text.
- **Unchanged.** The harness replaces the final component of each temporary
  directory with `$DIR`. The two processes get different directories. Both
  programs print the real path they were given. A different drive, slash,
  or a missing `.swarmnotes` segment still fails the comparison.
- **Unchanged.** For the init path checks, the harness calls Examen's
  `examen_init` function in-process and calls Relatum through the
  `relatum_init` tool. It drops a leading `Error executing tool ...:` from
  the tool text so that wrapper is not part of the comparison. The refusal
  is the same rule: create a store only in the working directory or at
  `SWARMNOTES_DIR`.

After those rewrites, the harness compares CLI exit codes, stdout, stderr,
and the note files. The note format is still the v0.2 format.

## CLI

```sh
relatum init                                  # .swarmnotes/ + default report format
relatum new net/retry-decision --kind decision \
  --summary "Exponential backoff, max 4 attempts" --author parent --body "..."
relatum ls net --kind decision                # id, kind, summary
relatum get net/retry-decision _shared/report-format
relatum supersede net/backoff-linear net/retry-decision
relatum check                                 # lint
```

Set `SWARMNOTES_DIR` to point at a store without walking up from the working
directory. `ls` hides superseded notes unless `--all`. `get` follows
`superseded_by`, including multi-hop chains. `swarmnotes` accepts the same
arguments and prints that name in help and errors.

## MCP server

`relatum-mcp` is a stdio MCP server over **the same files** as the CLI. There
is no second store and no hosted service. `relatum mcp` is the same process.
`npx -y @iron_adamant/relatum mcp` is the same process from the npm package.
Hosts that speak MCP (Claude Code, Claude Desktop, Cursor, Grok Build, and
other MCP clients) launch one of those commands. The registry manifest is
[`server.json`](server.json) (`io.github.IronAdamant/relatum`).

Tools: `relatum_init`, `relatum_ls` (scope, kind, include_superseded, tag),
`relatum_get` (one or more IDs; follows superseded notes), `relatum_new`,
`relatum_supersede`, `relatum_check`.

The server finds `.swarmnotes/` the same way the CLI does (walk up from the
working directory, or `$SWARMNOTES_DIR`). Set `SWARMNOTES_DIR` to the absolute
store path when the host does not start the process inside the project.
`relatum init` creates the directory. `relatum_init` does that only for the
server's current working directory, or for `SWARMNOTES_DIR` (the store
itself, or the project directory that contains it). Any other path is an error.

### Claude Code and Claude Desktop

Project `.mcp.json` (Claude Code) or `claude_desktop_config.json`
(Claude Desktop):

```json
{
  "mcpServers": {
    "relatum": {
      "type": "stdio",
      "command": "relatum-mcp",
      "args": [],
      "env": {
        "SWARMNOTES_DIR": "/absolute/path/to/project/.swarmnotes"
      }
    }
  }
}
```

Claude Code can also add it from the shell: `claude mcp add relatum -- relatum-mcp`.

### Cursor

`.cursor/mcp.json` in the project, or `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "relatum": {
      "command": "relatum-mcp",
      "args": [],
      "env": {
        "SWARMNOTES_DIR": "/absolute/path/to/project/.swarmnotes"
      }
    }
  }
}
```

### Grok Build

Grok Build reads stdio servers from `~/.grok/config.toml`, or from
`.grok/config.toml` in the project when you want the config to travel with
the repo. This matches the stdio shape in the Grok Build MCP docs
(`command`, `args`, `env`): https://docs.x.ai/build/features/mcp-servers

```toml
[mcp_servers.relatum]
command = "relatum-mcp"
args = []
env = { SWARMNOTES_DIR = "/absolute/path/to/project/.swarmnotes" }
```

From the shell, the same server is:

```sh
grok mcp add relatum --scope project \
  -e SWARMNOTES_DIR=/absolute/path/to/project/.swarmnotes \
  -- relatum-mcp
```

`--scope project` writes `.grok/config.toml`. Omit it to write
`~/.grok/config.toml`. Grok also imports Cursor and Claude MCP config, so a
project that already has the Cursor snippet above is visible there too.

Prefer the MCP tools when the host exposes them; otherwise use the CLI or
the files. See [AGENTS.snippet.md](AGENTS.snippet.md).

## Design principles

- **The parent decides what is relevant.** Briefs name note IDs; children
  do not have to guess from a catalog.
- **Scopes keep listings small.** Each branch of a swarm gets its own
  directory, so `ls` stays cheap at thousands of notes.
- **Notes are facts, not documents.** One-line summary, body under about 300 words.
- **Nothing silently goes stale.** Replaced notes are marked superseded, and
  `get` follows the link.
- **Write-back is part of the protocol.** Children's findings and failed
  attempts are notes too.

## Status

1.0.1. The note format is the v0.2 format, now treated as stable. The CLI
and MCP server are the Rust port of Examen 0.3.0. See
[CHANGELOG.md](CHANGELOG.md). The previous Python tree is the `python-legacy`
branch.

## Tests

```sh
cargo test --locked
python3 parity/harness.py target/release/relatum target/release/relatum-mcp
```

The harness checks this binary against Examen 0.3.0 on `python-legacy`.
Fetch that branch first if it is not already local.

## Benchmarks

Measured on Linux 6.12.94+ x86_64, 4 vCPU, Python 3.12.3 against a Relatum
1.0.0 release build (rustc 1.83.0). Figures are medians. One warmup run is
dropped. Python and Relatum alternate which starts first. The small store
has 30 notes, the large store 2000, and the big check store 8000, each plus
the report note. `ls`, `get`, and `check` read that store. `new` and
`supersede` run on a copy. The MCP row is one process start, an initialize
request, and one `ls` call. Memory is the peak resident set (`VmHWM`) of
`check` on the big store. Lower is better on every row.

| Row | Python 0.3.0 | Relatum 1.0.0 |
|---|---:|---:|
| cli startup (`--help`) | 25.9 ms | 0.7 ms |
| mcp startup + ls round trip | 457.1 ms | 1.4 ms |
| init empty | 25.0 ms | 1.0 ms |
| init on large store | 24.7 ms | 0.9 ms |
| ls small | 26.6 ms | 1.0 ms |
| get small | 24.8 ms | 0.9 ms |
| check small | 26.7 ms | 1.0 ms |
| ls large | 103.2 ms | 9.8 ms |
| get large | 24.8 ms | 0.9 ms |
| check large | 118.5 ms | 10.4 ms |
| new small | 24.9 ms | 0.8 ms |
| new large | 24.7 ms | 0.9 ms |
| supersede small | 25.0 ms | 0.9 ms |
| supersede large | 25.9 ms | 0.8 ms |
| check big | 402.1 ms | 39.8 ms |
| memory check big (RSS) | 22940 KB | 4044 KB |

`parity/bench.py` repeats the measurement. It reads `python-legacy`.

## GitHub topics

Topics are set in the repository settings, not from this tree. Suggested:
`ai-agents`, `mcp`, `model-context-protocol`, `llm`, `claude`, `cursor`,
`grok`, `rust`, `agent-tools`.

## License

MIT. See [LICENSE](LICENSE). Contributing notes are in [CONTRIBUTING.md](CONTRIBUTING.md).
