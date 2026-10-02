---
kind: finding
summary: httpx.ReadTimeout isn't a ConnectError subclass; it must be listed explicitly.
author: child-client
updated: 2026-10-03
tags: [net, httpx]
---

`httpx.ConnectError` and `httpx.ReadTimeout` share only `httpx.TransportError`.
Catching `ConnectError` alone silently skips retries on read timeouts. The
client now catches `(httpx.ConnectError, httpx.ReadTimeout, httpx.RemoteProtocolError)`.
