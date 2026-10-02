---
kind: decision
summary: Exponential backoff with full jitter, max 4 attempts, idempotent requests only.
author: parent
updated: 2026-10-03
tags: [net, retry]
---

Base delay 200ms, factor 2, full jitter, max 4 attempts.
Retry on connection errors and 502/503/504 only. Never retry POST/PATCH unless
the request has an `Idempotency-Key` header.

Why: the upstream rate-limits bursts; linear backoff caused thundering herds
(see net/backoff-linear).
