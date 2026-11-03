# Changelog

## 0.3.0 — 2026-10-02

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

## 0.2.0 — 2026-10-02

- Public CLI: `init`, `new`, `ls`, `get`, `supersede`, `check`.
- `swarmnotes` is an alias of `examen`.
- Pass-by-reference note format ([SPEC.md](SPEC.md)). Notes live in
  `.swarmnotes/`.
- `ls` hides superseded notes unless `--all`. `get` follows `superseded_by`,
  including multi-hop chains.
- Lint fails on broken frontmatter, bad kinds, summaries, tags, dates, empty
  bodies, and supersede chains that do not end on a live note. Bodies over
  300 words warn.
