# Contributing

The note format in [SPEC.md](SPEC.md) is the product. Briefs stay
`Task` / `Read first` / `Your scope` / `Done when`. The CLI and the MCP
server are thin conveniences over the same `.swarmnotes/` files.

- Rust standard library only. `Cargo.toml` has no dependencies and no
  dev-dependencies. Do not add a crate.
- The package `name` in `Cargo.toml` is the rename point for the CLI, the
  `{name}-mcp` binary, and the `{name}_*` tools. `swarmnotes` stays as the
  legacy alias.
- Do not rename `.swarmnotes/`. `SWARMNOTES_DIR` stays the environment
  variable. Stores written by Examen 0.3.0 must keep loading.
- No hosted service, account, or second store.
- Do not publish to crates.io or PyPI. `publish` stays false in Cargo.toml.
  npm packages under `npm/` are published by the release workflow.
- Docs use Australian spelling and plain ASCII.

## Tests

```sh
cargo test --locked
cargo build --release --locked
cargo run --release --locked --example parity -- target/release/relatum target/release/relatum-mcp
```

On Windows add `.exe` to both binary paths. Without MSVC build tools, use
the `x86_64-pc-windows-gnu` toolchain; if another MinGW `gcc` is on `PATH`,
build with `RUSTFLAGS="-C link-self-contained=yes"` so Rust links with its
bundled linker.

## Parity with Examen 0.3.0

`parity/harness.rs` (the `parity` example) runs this binary and Examen 0.3.0
side by side and exits 1 on any difference. `parity/bench.rs` (the `bench`
example) prints the benchmark table in the README; its memory row reads
`/proc`, so run it on Linux. Both are std only. The frozen Python release is
the reference oracle: the harness loads `examen.py` from the
`python-legacy` branch (`git fetch origin python-legacy`) into a Python 3
process, and runs `python -m examen_mcp`, which needs
`pip install "mcp==2.3.0"`. Set `PARITY_PYTHON` to choose the interpreter;
otherwise `python3`, then `python`. Nothing uses the network after that.
GitHub Actions runs `cargo test` and the harness on Linux, macOS, and
Windows.

The harness compares CLI exit codes, stdout, stderr, and note file bytes,
and the MCP responses, stderr log, and files. It normalises only these:

- Examen's program name is forced to `relatum` or `swarmnotes`. MCP text
  has `relatum_` rewritten to `examen_`, then `Relatum` to `Examen`, then
  `relatum` to `examen`. `1.0.1` and `1.0.0` become `0.3.0`
  (`serverInfo.version`).
- **Intentional difference: argument validation errors.** Examen printed
  pydantic's multi-line report with documentation URLs. Relatum prints one
  line, `invalid arguments: <field> must be <type>, got <value>. Call
  <tool> again with <field> like <example>.` or `invalid arguments:
  missing required argument(s) <fields>. <tool> needs <all required>.`
  The harness compares these by shape only: an error result whose text is
  pydantic's report on one side and one `invalid arguments:` line on the
  other becomes the same placeholder. `isError`, the tool name, and the
  `rejected arguments: [...]` line both servers log to stderr (the exact
  fields) are still compared. CLI argparse errors are unchanged.
- The final component of each temp directory becomes `$DIR`.
- CR and LF are folded on CLI pipes and MCP stderr. On Windows both
  programs write CRLF there, as CPython's text-mode streams do.
- `relatum_ls`'s opt-in `limit`, `since`, and `details` arguments are
  dropped from its input schema. `relatum ls -h` lists the matching CLI
  flags; its usage line, which argparse errors print, is unchanged.
- MCP tool descriptions are compared after docstring dedent. Relatum sends
  them unindented; Examen's carry Python 3.12 docstring indentation (3.13
  strips it).
- A tool result's JSON text is compared as data. Relatum writes it compact;
  Examen indented it. `structuredContent` is compared unchanged.
- Relatum's MCP errors add a next step for a missing store, a scope with no
  notes, and an invalid id. The session the harness drives does not hit
  those three cases; `tests/mcp_agent.rs` covers them.
- For the `relatum_init` path rules, Examen's `examen_init` is called
  in-process and a leading `Error executing tool ...:` is dropped from
  Relatum's tool text.

`init` still writes `author: examen` on `_shared/report-format`, so a store
created on the same local date matches byte for byte.
