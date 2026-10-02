# Example: a parent adds retry logic with three children

The parent has made a few decisions about a fictional `acme` HTTP client and
wants three children to work in parallel. The notes they share are in
[`.swarmnotes/`](.swarmnotes/). Examen reads and lints that directory. The
storage directory stays `.swarmnotes/`.

## 1. Parent writes notes once

```
_shared/report-format     format      Shape of the final report every agent returns.
_shared/repo-conventions  convention  Python 3.11, ruff + pytest, src/ layout, type hints...
net/retry-decision        decision    Exponential backoff with full jitter, max 4 attempts...
net/no-new-deps           constraint  No new third-party dependencies; implement retry...
```

(`net/backoff-linear` is an earlier decision, superseded by `net/retry-decision`.
It's hidden from `ls` by default, and `get` on it follows the link.)

## 2. Parent spawns children with briefs

Each brief names note IDs instead of pasting them.

**child-client**
```
Task: add retry logic to src/acme/net/client.py per the retry decision.
Read first: _shared/report-format, _shared/repo-conventions, net/retry-decision, net/no-new-deps
Your scope: net/client
Done when: client retries per the decision; existing tests still pass.
```

**child-tests**
```
Task: write tests for the retry behaviour in tests/net/test_retry.py. Tests must not actually sleep.
Read first: _shared/report-format, _shared/repo-conventions, net/retry-decision
Your scope: net/tests  (also read net/client for the client agent's findings)
Done when: tests cover retry-on-503, no-retry-on-POST, and max attempts.
```

**child-docs**
```
Task: document retry behaviour in docs/networking.md (one short section).
Read first: _shared/report-format, net/retry-decision
Your scope: net/docs
Done when: section exists and matches the decision.
```

Without notes, the parent would paste the conventions, decision, constraint
and report format into each of these briefs. That's the same text written up
to three times, and all of it stays in the parent's context.

## 3. Children write back

`child-client` found something a sibling needs to know:

```
net/client/timeout-not-retried  finding  httpx.ReadTimeout isn't a ConnectError subclass...
```

`child-tests` lists `net/client`, sees that finding, and covers read timeouts
too. It also records a dead end so nobody repeats it:

```
net/tests/attempt-freezegun  attempt  freezegun doesn't stop time.sleep; patch the client's sleep...
```

## 4. Reports stay short

```
Status: done
Changed: tests/net/test_retry.py (new), src/acme/net/client.py (sleep param)
Notes written: net/tests/attempt-freezegun
Open questions: none
```

The parent reads three reports like this and only fetches a cited note if it
needs the detail.

## Try it

```sh
cd examples/retry-swarm
python3 ../../examen.py ls
python3 ../../examen.py ls net --kind decision --all
python3 ../../examen.py get net/backoff-linear
python3 ../../examen.py check
```

## MCP smoke

The MCP server reads this same directory. From the repo root, with no extra
installed, the tool handlers are plain functions:

```sh
python3 -c '
import os
from examen_mcp import examen_check, examen_get, examen_ls
os.chdir("examples/retry-swarm")
print(examen_check())
print(examen_ls("net", include_superseded=True))
print(examen_get(["net/backoff-linear"]))
'
```

`examen_get` on `net/backoff-linear` follows the link to `net/retry-decision`.

To speak MCP, install the optional extra and leave the process on stdio (that
is what Claude Code and Cursor launch):

```sh
pip install "examen[mcp]"
cd examples/retry-swarm
examen-mcp
```

Set `SWARMNOTES_DIR` to this `.swarmnotes/` path if the host does not start
the process inside the example directory. Config snippets are in the README.
