---
title: AP.3 — Authenticated outbound SSE session
status: planned
recommended_agent: arch-ctm
branch: plan/phase-ao-tls-and-ap-outbound-connectivity
worktree: ../atm-core-worktrees/plan/phase-ao-tls-and-ap-outbound-connectivity
---

# AP.3 — Authenticated outbound SSE session

## Scope

Implement one outbound mTLS SSE session from the restricted host to the
approved reachable endpoint and one bounded in-memory registry on the
reachable host. The authenticated TLS peer identity owns registry lookup;
neither HTTP headers nor raw IP assign ownership.

The session's only TLS call path is the AO.2-installed opaque
`atm_http_runtime::PeerIoAdapter`: its outbound GET/POST connection uses
`PeerIoAdapter::connect`, and the reachable side accepts the same session with
`PeerIoAdapter::accept`. The adapter returns `BoxedPeerIo`, which the existing
Hyper HTTP/1 service consumes. AP.3 must not construct a Rustls client,
connector, or TLS configuration of its own.

```text
Connecting -- mTLS verifies --> Authenticated(peer) -- register --> Live(peer)
Live -- disconnect/ownership change --> Closed -- remove registry entry
```

## Dependencies

- **must_follow:** AP.2 PR merged.
- **parallel_safe:** none. AP.4 consumes the live session.
- **unblocks:** AP.4.

## Deliverables

1. HTTP/1.1 SSE session establishment over the AO.3 mTLS transport.
2. Bounded live-session registry with one owner/generation per authenticated
   hostname and deterministic replacement/disconnect cleanup.
3. Heartbeat and reconnect as lifecycle signals only, never delivery retry.
4. Negative handling for wrong/disabled certificate, forged headers, duplicate
   session, stale generation, overflow, and disconnect.

## Acceptance criteria

- Only the current authenticated live session can be selected by hostname.
- No live session exists after disconnect or ownership replacement.
- Stream overflow and unavailable session produce typed failures without
  storing, retrying, or replaying a request.

## Required validation

- Async lifecycle tests with a bounded in-memory registry.
- mTLS SSE integration tests for identity, replacement, disconnect, and
  capacity negative cases.
- `just lint` and `just test`.

## Non-closure

AP.3 does not carry canonical ATM writes or return correlated results.
