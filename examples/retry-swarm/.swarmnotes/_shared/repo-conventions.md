---
kind: convention
summary: Python 3.11, ruff + pytest, src/ layout, type hints on public functions.
author: parent
updated: 2026-10-03
---

- Code lives in `src/acme/`; tests mirror it under `tests/`.
- Run `ruff check . && pytest -q` before reporting done.
- Public functions get type hints and a one-line docstring.
- No print debugging left in; use `logging.getLogger(__name__)`.
