# Relatum

Formerly Examen.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Pass-by-reference notes for agent swarms.**

When an agent spawns subagents, it usually pastes the same decisions,
constraints, and conventions into every spawn prompt. Relatum lets the
parent write each fact **once** as a short note, then send each child a
brief that names note IDs:

```
Task: add retry logic to src/acme/net/client.py per the retry decision.
Read first: _shared/report-format, _shared/repo-conventions, net/retry-decision
Your scope: net/client
Done when: client retries per the decision; existing tests still pass.
```

Children read what they are told to, write findings and dead ends back as
notes, and return short reports that cite note IDs. Siblings see each
other's findings, and a later session can pick the work up cold from the
same files.

Relatum is one small Rust binary (standard library only, no Cargo crates) with a
CLI and a stdio MCP server over the same files. No service, no account.

## How it works

```
.swarmnotes/
  _shared/report-format.md       <- id: _shared/report-format
  net/retry-decision.md          <- id: net/retry-decision (written by the parent)
  net/client/timeout-bug.md      <- written back by a child in scope net/client
```

- A note is markdown with a little frontmatter (`kind`, `summary`,
  `author`, `updated`, `tags`). Its ID is its path.
- Parents write `decision`, `constraint`, `convention`, `context`, and
  `format` notes. Children write `finding` and `attempt` notes in their own
  scope.
- Creating a note never overwrites one. To change your mind, write a new
  note and `supersede` the old ID; `get` on the old ID follows the chain.
- Every write lands whole, so agents in parallel never read a half-written
  note.
- `check` lints the store.

The store is the nearest `.swarmnotes/` walking up from the working
directory, or `$SWARMNOTES_DIR`. Any agent that can read files can take
part, with or without the tools. The full format, the parent/child
protocol, and the lint rules are in **[SPEC.md](SPEC.md)**.

## Install

```sh
npx -y @iron_adamant/relatum --help       # npm: prebuilt binary for this OS
```

Or download an archive for Linux or macOS (x64, arm64) or Windows (x64) from
[Releases](https://github.com/IronAdamant/Relatum/releases) and check it
against `SHA256SUMS`. Or build it (Rust 1.83 or newer):

```sh
git clone https://github.com/IronAdamant/Relatum
cd Relatum
cargo build --release --locked   # target/release/relatum and relatum-mcp
```

Each install gives `relatum` (CLI), `relatum-mcp` (MCP server; `relatum mcp`
is the same), and `swarmnotes` (the old CLI name). Relatum is not on
crates.io or PyPI; the unscoped `examen` packages there and on npm are
unrelated projects.

## MCP setup

Claude Code (`.mcp.json` in the project) or Claude Desktop
(`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "relatum": {
      "type": "stdio",
      "command": "relatum-mcp",
      "args": [],
      "env": { "SWARMNOTES_DIR": "/absolute/path/to/project/.swarmnotes" }
    }
  }
}
```

From the shell: `claude mcp add relatum -- relatum-mcp`. With npm only, use
`"command": "npx", "args": ["-y", "@iron_adamant/relatum", "mcp"]`.
`SWARMNOTES_DIR` is needed only when the host does not start the server
inside the project. Cursor (`.cursor/mcp.json`) takes the same JSON without
`"type"`. Grok Build (`.grok/config.toml`):

```toml
[mcp_servers.relatum]
command = "relatum-mcp"
args = []
env = { SWARMNOTES_DIR = "/absolute/path/to/project/.swarmnotes" }
```

Tools: `relatum_init`, `relatum_ls`, `relatum_get`, `relatum_new`,
`relatum_supersede`, `relatum_check`. Results are compact JSON, and errors
say what to call next. To teach agents the protocol, paste
[AGENTS.snippet.md](AGENTS.snippet.md) into the project's `CLAUDE.md` or
`AGENTS.md`, or install [skills/relatum](skills/relatum/SKILL.md).

## Example

```sh
relatum init
relatum new net/retry-decision --kind decision --author parent \
  --summary "Exponential backoff, max 4 attempts" \
  --body "Base 200ms, factor 2, full jitter. Retry only 502/503/504."

# a child, in any subdirectory, later or in parallel
relatum get net/retry-decision _shared/report-format
relatum new net/client/timeout-bug --kind finding --author worker-a \
  --summary "Timeouts were not retried" --body "client.py:88 caught them first."

# the parent
relatum ls net                     # id, kind, summary
relatum new net/retry-v2 --kind decision --author parent \
  --summary "Max 6 attempts" --body "Upstream raised its limit."
relatum supersede net/retry-decision net/retry-v2
relatum check
```

[examples/retry-swarm](examples/retry-swarm/WALKTHROUGH.md) walks through a
parent and three children. [templates/](templates/) has a blank brief and
note.

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

`cargo run --release --locked --example bench -- target/release/relatum
target/release/relatum-mcp` repeats the measurement. It reads
`python-legacy`.

## Coming from Examen

Examen 0.3.0 (Python) is frozen on the `python-legacy` branch. Relatum
reads the same `.swarmnotes/` files and honours `SWARMNOTES_DIR`, with no
migration. The CLI is `relatum` and the MCP tools are `relatum_*` instead
of `examen_*`. MCP argument errors are one line that says what to send,
instead of pydantic's report. The parity harness checks this build
against Examen; see
[CONTRIBUTING.md](CONTRIBUTING.md). Changes are in
[CHANGELOG.md](CHANGELOG.md).

## License

MIT. See [LICENSE](LICENSE).
