---
title: AP.1 — Real corporate-network feasibility proof
status: planned
recommended_agent: arch-ctm
---

# AP.1 — Real corporate-network feasibility proof

## Scope

Prove or honestly block the proposed outbound-only route on the actual CWin,
M4, and M5 machines before adding product transport code. CWin must initiate
an mTLS HTTP/1.1 SSE connection to a nominated reachable M4 endpoint, receive
a correlated event, and submit an ordinary authenticated POST response.

```text
CWin -- outbound mTLS GET /peer-session (SSE) --> M4
M4  -- correlated event over that SSE ----------> CWin
CWin -- outbound mTLS POST /peer-session/result -> M4
```

## Dependencies

- **must_follow:** none.
- **parallel_safe:** no AP product sprint; this decides whether AP is viable.
- **unblocks:** AP.2 only on PASS.

## Deliverables

1. Freeze candidates/versions and record safe host network observations: DNS,
   route, proxy configuration, and policy result, never credentials.
2. A non-product proof harness with the exact mTLS/SSE/POST sequence above,
   one deliberate disconnect/reconnect, and bounded observation interval.
3. XHTML and machine-readable report artifacts beneath `site/reports/` plus
   master-index verification.

## Acceptance criteria

- PASS requires CWin-initiated mTLS connection, correlated event receipt,
  authenticated POST result, and reconnection through the same real policy.
- If policy prevents this, the precise observable failure is retained as
  BLOCKED and AP.2–AP.5 do not start.
- No tunnel, relay, localhost, alternative host, or raw-IP substitute counts.

## Required validation

- Candidate SHA and command capture on every host.
- DNS/TCP/TLS/SSE/POST/reconnect matrix and safe report-index check.
- `/smoke-test` conventions for report paths and machine identity.

## Non-closure

AP.1 does not add a live daemon endpoint or alter ATM delivery behavior.
