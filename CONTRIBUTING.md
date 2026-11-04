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
- Do not publish to crates.io, PyPI, or npm. `publish` stays false.
- Docs use Australian spelling and plain ASCII.

## Tests

```sh
cargo test --locked
cargo build --release --locked
python3 parity/harness.py target/release/relatum target/release/relatum-mcp
```

The harness needs the `python-legacy` branch
(`git fetch origin python-legacy`) and, for the MCP half,
`pip install "mcp==2.3.0"`. It does not use the network after that.
GitHub Actions runs `cargo test` and the harness on Linux, macOS, and Windows.
