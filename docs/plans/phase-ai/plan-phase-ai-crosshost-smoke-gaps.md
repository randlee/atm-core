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
creating a second transport path or delivery-state subsystem. A peer write has
one canonical local write path; HTTPS is only its post-write adapter.

## Findings and owners

| Finding | Owner sprint | Closure |
| --- | --- | --- |
| IP-keyed trust becomes stale; hostname input does not authorize current IP | AI.22 | DNS-backed hostname authority with certificate pin |
| local 3s deadline conflicts with independent 5s peer legs | AI.23 | one propagated absolute deadline and cancellation ownership |
| local timeout is falsely `DAEMON_UNAVAILABLE`; handler errors disappear | AI.24 | typed uncertainty result and retained terminal events |
| persistence event claims `sent` before peer acceptance | AI.24 | truthful delivery event contract |
| Wi-Fi/VPN loss leaves recent persisted writes without recovery | AI.25 | bounded per-peer reconciliation schedule with backoff |
| smoke treated local `outcome sent` as receipt | AI.26 | receiver-side ULID evidence and physical rerun |

The TLS trust-snapshot/restart observation is closed by AI.22's daemon-owned
atomic configuration refresh. The five Windows sends in the first smoke remain
unconfirmed; none is release evidence.

## Dependencies

```text
AI.22 peer authority ─┐
AI.23 deadline budget ─┼─> AI.25 bounded recovery ─> AI.26 physical rerun
AI.24 outcome truth ───┘
```

AI.22, AI.23, and AI.24 may be implemented in parallel because they own
separate authority, deadline, and observability seams. AI.25 depends on
AI.23/AI.24; AI.26 starts after AI.22–AI.25 merge.

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
- Every repeated immutable ULID follows the same write path and remains
  idempotent. No finding authorizes an outbox, replay queue, receipt, retry
  worker, or parallel acknowledgement workflow.

## Required validation

Each sprint runs `just lint` and `just test`. AI.26 additionally requires
receiver-side evidence for each ULID, matching client/daemon versions, doctor
before/after, exactly one daemon per host, and sanitized logs. Raw TCP success,
local persistence, and a sender-side `sent` event are insufficient evidence.
