# Changelog

## Unreleased

- Notes are written whole: a temp file is hard-linked (new) or renamed
  (supersede) into place, so a concurrent reader never sees a torn note.
  On Windows the replace uses POSIX rename semantics, so busy readers no
  longer starve a writer on Rust 1.83.
- MCP tool descriptions drop docstring indentation, tool result text is
  compact JSON, and errors for a missing store, an empty scope, and an
  invalid id say what to call next. The CLI is unchanged.
- The parity harness and benchmark are std-only Rust (`cargo run --example
  parity` / `bench`); the Python scripts are removed.
- README rewritten for newcomers; parity details moved to CONTRIBUTING.md.
- `ls` and `check` read large stores on up to eight threads: on Windows a
  2000-note `ls` or `check` drops from about 150 ms to about 55 ms.
- `ls --limit N`, `--since TIME`, and `--details` (and `relatum_ls` `limit`,
  `since`, `details`): opt-in, default output unchanged.
- `relatum_edit`: a note's author corrects its summary, body, or tags in
  place. Errors for an existing id point to `relatum_edit` and
  `relatum_supersede`.
- MCP argument errors are one line (field, value, expected, how to call
  again) instead of pydantic's report. Intentional difference from Examen.
- The bench's memory row works on Windows and macOS; CI runs the bench on
  all three OSes and checks the table.

## 1.0.1 - 2026-11-04

- npm packages `@iron_adamant/relatum` and five platform packages
  (`linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`, `windows-x64`).
  The launcher has no npm dependencies. `npx -y @iron_adamant/relatum mcp`
  starts the stdio server.
- `relatum mcp` starts the same stdio server as `relatum-mcp`.
- `server.json` names the server `io.github.IronAdamant/relatum` for the MCP
  Registry.
- The `v*` release workflow still attaches the five archives and
  `SHA256SUMS` to the GitHub Release, then publishes the npm packages. That
  job requires the `NPM_TOKEN` repository secret and fails if it is missing.
  `workflow_dispatch` can publish npm for an existing tag without rebuilding.
- README section "Migrating from Examen (Python)".

## 1.0.0 - 2026-11-04

- Rust port of the Examen 0.3.0 CLI and stdio MCP server. No Cargo crates.
  The note format is unchanged from v0.2 and is treated as stable.
- The crate, the CLI, and the MCP server are named `relatum`. Tools are
  `relatum_init`, `relatum_ls`, `relatum_get`, `relatum_new`,
  `relatum_supersede`, and `relatum_check`. `swarmnotes` remains a CLI alias.
- `.swarmnotes/` and `SWARMNOTES_DIR` are unchanged, so existing stores load
  with no migration. `init` still seeds `author: examen` on
  `_shared/report-format`.
- The Python implementation is the `python-legacy` branch. This tree does
  not publish to crates.io, PyPI, or npm.
- Release workflow on `v*` tags builds linux x64, linux arm64, macOS x64,
  macOS arm64, and Windows x64 binaries with `SHA256SUMS`.

## 0.3.0 - 2026-10-02

- Optional stdio MCP server over the same `.swarmnotes/` files as the CLI.
  Install from GitHub with
  `pip install "examen[mcp] @ git+https://github.com/IronAdamant/Examen"`
  (official MCP Python SDK, Python 3.10+). This project is not published on
  PyPI; that name belongs to a different package. The core `examen` command
  stays zero-dependency.
- `examen_init` creates a store only in the current working directory or at
  `SWARMNOTES_DIR`.
- Tools: `examen_init`, `examen_ls`, `examen_get`, `examen_new`,
  `examen_supersede`, `examen_check`.
- Console script `examen-mcp`, or `python -m examen_mcp`.
- Agent skill and `AGENTS.snippet.md` prefer the MCP tools when a host
  exposes them.

## 0.2.0 - 2026-10-02

- Public CLI: `init`, `new`, `ls`, `get`, `supersede`, `check`.
- `swarmnotes` is an alias of `examen`.
- Pass-by-reference note format ([SPEC.md](SPEC.md)). Notes live in
  `.swarmnotes/`.
- `ls` hides superseded notes unless `--all`. `get` follows `superseded_by`,
  including multi-hop chains.
- Lint fails on broken frontmatter, bad kinds, summaries, tags, dates, empty
  bodies, and supersede chains that do not end on a live note. Bodies over
  300 words warn.
