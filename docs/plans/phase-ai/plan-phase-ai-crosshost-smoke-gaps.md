---
title: Phase AI cross-host smoke-gap closure
status: proposed
branch: plan/phase-ai-crosshost-smoke-gaps
worktree: ../atm-core-worktrees/plan/phase-ai-crosshost-smoke-gaps
target: develop then integrate/phase-AI
depends_on: AI.11–AI.16
---

# Phase AI — cross-host smoke-gap closure

## Goal

Close the concrete defects found in the first Mac↔Windows smoke without
creating a second transport path or delivery-state subsystem. HTTPS remains
only the post-write adapter of the existing canonical write path.

## Findings and owners

| Finding | Owner sprint | Closure |
| --- | --- | --- |
| IP-keyed trust becomes stale; hostname input does not authorize current IP | AI.22 | DNS-backed hostname authority with certificate pin |
| local 3s deadline conflicts with independent 5s peer legs | AI.23 | one propagated absolute deadline and cancellation ownership |
| local timeout is falsely `DAEMON_UNAVAILABLE`; handler errors disappear | AI.24 | typed uncertainty result and retained terminal events |
| persistence event claims `sent` before peer acceptance | AI.24 | truthful delivery event contract |
| Wi-Fi/VPN loss leaves recent persisted writes without recovery | AI.25 | per-peer connection lock with release-then-check reconciliation loop |
| smoke treated local `outcome sent` as receipt | AI.26 | receiver-side ULID evidence and physical rerun |
| product releases block compatible CLI/daemon pairs | AI.27 | explicit schema and HTTP SemVer compatibility contract |

The TLS trust-snapshot/restart observation is closed by AI.22's daemon-owned
atomic configuration refresh. The five Windows sends in the first smoke remain
unconfirmed; none is release evidence.

## Dependencies

```text
AI.22 peer authority ──────────────────────────────┐
AI.23 deadline/error contract ─> AI.24 outcome truth ─> AI.25 recovery ─> AI.26 physical rerun
AI.27 schema/HTTP compatibility ────────────────────┘
```

AI.22 and AI.23 may be implemented in parallel because they own separate
authority and deadline seams. AI.24 follows AI.23 because it reports AI.23's
typed uncertainty result. AI.25 follows AI.24. AI.26 starts only after
AI.22–AI.25 and AI.27 merge. AI.27 may proceed in parallel with AI.22–AI.25,
so tested
CLI/daemon builds are not artificially blocked by release-label drift.

## Invariants

- A registered hostname plus certificate pin is the peer authority; resolved
  IPs are transient DNS facts, never SQLite aliases.
- An IP target is accepted only when it currently resolves from exactly one
  registered hostname. No reverse-DNS inference exists.
- A single absolute request deadline governs every local and remote leg.
- A remote write is successful only after peer HTTP acceptance; local
  persistence is separately observable, never proof of receiver receipt.
- A failed peer write may schedule bounded reconciliation of existing immutable
  records, but it may not create a per-message queue, receipt, or retry state.
- AI.25 gates outbound connection attempts (never local writes) behind a
  per-peer connection lock:
  1. **Connect, then read the backlog, then send it as one batch.** The
     backlog is read only after the connection succeeds, not before, so any
     record written during the connect attempt is still captured. Justification:
     reading first risks missing stragglers written mid-attempt.
  2. **The connect attempt is the only reachability signal.** No separate
     probe: connect uses a non-blocking socket with an explicit bounded
     deadline (not the OS default SYN-retry timeout, ~127s macOS/Linux,
     ~21s Windows), so a failed attempt tears down its socket immediately.
     ICMP is never used, even though the two current hosts (fastpc4, m5)
     answer ping today: ping confirms IP-layer reachability, not that the
     peer's ATM daemon is listening on its registered port, which is the
     only fact that matters here. No OS interface-change listener and no
     other application-level event (e.g. an SSH session succeeding)
     triggers reconciliation, since either would only be a proxy for what
     the connect attempt already establishes directly.
  3. **On success: release the lock, then check the backlog again — never
     check before releasing.** If it's non-empty, reacquire and repeat; if
     empty, stop. Justification: checking before release always leaves a
     gap between "found nothing" and "actually released" during which a
     new record can land locked out and unwatched, since the task has
     already committed to stopping and won't recheck. Checking after
     release removes that gap entirely — the two states ("nothing to do"
     and "lock still held") can never coexist, so a writer arriving at that
     instant simply acquires the free lock itself. Whichever of the
     finishing task's reacquire or a new writer's acquire wins the resulting
     race is immaterial; both outcomes correctly service the record.
  4. **On failure: release the lock and set/advance a per-peer backoff
     timer.** No immediate retry.
  5. **Writers never block on the lock.** After persisting locally, a writer
     opportunistically tries to acquire the peer's lock; if free, it runs
     the send cycle itself (this is what wakes a fully idle peer with no
     task and no pending backoff); if held, it does nothing further, since
     the running task's release-then-check step is guaranteed to observe
     the record.
  6. **At most one connection attempt in flight per peer, scoped per
     peer — never global.** fastpc4 and m5 retry independently. Combined
     with the bounded connect deadline in (2), this bounds socket and thread
     usage regardless of how many backoff cycles run.
- Every repeated immutable ULID follows the same write path and remains
  idempotent. No finding authorizes an outbox, replay queue, receipt, retry
  worker, or parallel acknowledgement workflow.
- Product release SemVer is diagnostic only. A separate CLI/daemon schema and
  HTTP API SemVer contract governs admission; same-major additive HTTP changes
  remain interoperable.

## Required validation

Each sprint runs `just lint` and `just test`. AI.26 additionally requires
receiver-side evidence for each ULID, recorded CLI/daemon release plus
negotiated schema/HTTP API version, doctor before/after, exactly one daemon per
host, and sanitized logs. Raw TCP success, local persistence, and a sender-side
`sent` event are insufficient evidence.
