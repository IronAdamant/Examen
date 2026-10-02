# Contributing

The note format in [SPEC.md](SPEC.md) is the product. Briefs stay
`Task` / `Read first` / `Your scope` / `Done when`. The CLI and the MCP
server are thin conveniences over the same `.swarmnotes/` files.

- Keep the core CLI (`examen.py`) on the Python standard library.
  `dependencies` in `pyproject.toml` stays empty.
- The MCP SDK belongs only in the optional extra `examen[mcp]`. Do not import
  it from `examen.py`.
- Do not rename `.swarmnotes/`.
- No hosted service, account, or second store.

## Tests

```sh
python3 -m unittest discover -s tests
cd examples/retry-swarm && python3 ../../examen.py check
```

Core tests are unittest and do not need the `mcp` package. Handler tests in
`tests/test_examen_mcp.py` call the tool functions directly. The in-memory and
stdio checks in that file run when `examen[mcp]` is installed; they do not
use the network.
