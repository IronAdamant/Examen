---
kind: constraint
summary: No new third-party dependencies; implement retry with stdlib + existing httpx.
author: parent
updated: 2026-10-03
---

Do not add `tenacity`, `backoff`, `urllib3.Retry` or similar. The retry loop is
small enough to own, and the package ships to an air-gapped environment where
every dependency needs sign-off.
