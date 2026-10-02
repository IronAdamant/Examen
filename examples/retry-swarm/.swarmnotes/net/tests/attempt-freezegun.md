---
kind: attempt
summary: freezegun doesn't stop time.sleep; patch the client's sleep function instead.
author: child-tests
updated: 2026-10-03
---

Tried `freezegun` to make backoff tests instant; `time.sleep` still blocks
because freezegun only fakes clocks. Working approach: the client takes a
`sleep: Callable[[float], None] = time.sleep` parameter, and tests pass a fake
that records the delays.
