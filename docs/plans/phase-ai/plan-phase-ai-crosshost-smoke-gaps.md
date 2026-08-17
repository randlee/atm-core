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
creating a second transport path or delivery-state subsystem. Normal mTLS HTTPS
is only the post-write adapter of the existing canonical write path;
AI.21-pre's explicit plaintext-test process profile uses that same HTTP route
solely to diagnose connectivity and proves no production security property.

## Findings and owners

| Finding | Owner sprint | Closure |
| --- | --- | --- |
| mixed evidence branch has no supported, repeatable real-daemon harness | AI.21-pre | Python/sc-compose JSON/XHTML runner and explicitly labelled diagnostic wire profile |
| host-qualified same identity is rejected before remote routing | AI.22 | identity-only self-send guard; host-preserving address parser |
| peer/local write and nudge convergence is not mechanically proven | AI.23 | one HTTP `WriteRequest` endpoint and structural enforcement |
| host-qualified ACK reply has no receiver-inbox/nudge proof | AI.24 | advertised-IP TCP ACK receipt and nudge proof |
| IP-keyed trust becomes stale; hostname input does not authorize current IP | AI.25 | DNS-backed hostname authority with certificate pin |
| local 3s deadline conflicts with independent 5s peer legs | AI.26 | one propagated absolute deadline and cancellation ownership |
| local timeout is falsely `DAEMON_UNAVAILABLE`; handler errors disappear | AI.27 | typed uncertainty result and retained terminal events |
| persistence event claims `sent` before peer acceptance | AI.27 | truthful delivery event contract |
| Wi-Fi/VPN loss leaves recent persisted writes without recovery | AI.28 | bounded per-peer reconciliation schedule with backoff |
| smoke treated local `outcome sent` as receipt | AI.29 | receiver-side ULID evidence and physical rerun |
| product releases block compatible CLI/daemon pairs | AI.30 | explicit schema and HTTP SemVer compatibility contract |
| foreground peer delivery blocks local daemon response | AI.31 | SQLite-only local admission followed by non-durable peer-job signalling |
| recovery assumes an ordered peer stream | AI.32 | bounded independent ULID jobs with no cross-command ordering promise |
| no isolated 1,000/s proof or comprehensible two-host ladder | AI.33 | disposable-db capacity harness and ten-run local/cross-host HTML evidence |

The TLS trust-snapshot/restart observation is closed by AI.25's daemon-owned
atomic configuration refresh. The five Windows sends in the first smoke remain
unconfirmed; none is release evidence.

## Dependencies

```text
AI.21-pre evidence harness ─> AI.22 self-send guard ─> AI.23 shared write endpoint ─┬─> AI.24 ACK receipt proof
                                                                                       ├─> AI.25 peer authority ────────────┐
                                                                                       └─> AI.26 deadline/error contract ─> AI.27 outcome truth ─> AI.28 recovery
AI.30 schema/HTTP compatibility ───────────────────────────────────────────────┘
AI.24 + AI.25–AI.28 + AI.30 ─> AI.29 physical rerun ─> AI.31 local admission ─> AI.32 independent jobs ─> AI.33 capacity/smoke evidence
```

AI.21-pre closes the retained smoke harness/security-profile adoption before
the later behavioral sprints use it for real-daemon evidence. AI.22, AI.23,
and AI.24 are strictly ordered. AI.25 and AI.26 may be
implemented in parallel after AI.23 because they own separate authority and
deadline seams. AI.27 follows AI.26 because it reports AI.26's typed
uncertainty result. AI.28 follows AI.27. AI.29 starts only after AI.24,
AI.25–AI.28, and AI.30 merge. AI.30 may proceed in parallel with AI.25–AI.28,
so tested CLI/daemon builds are not artificially blocked by release-label drift.

## Invariants

- A registered hostname plus certificate pin is the peer authority; resolved
  IPs are transient DNS facts, never SQLite aliases.
- The existing `TrustedPeer { host, fingerprint, enabled }` evolves in AI.25
  by adding `https_port` while retaining `enabled` as the operator's allow/
  revoke control; a second `PeerAuthority` DTO is prohibited.
- An IP target is accepted only when it currently resolves from exactly one
  registered hostname. No reverse-DNS inference exists.
- A single absolute request deadline governs every local and remote leg.
- A remote write is successful only after peer HTTP acceptance; local
  persistence is separately observable, never proof of receiver receipt.
- A failed peer write may schedule bounded non-durable work over existing
  immutable records. Its transient queue may reference a hostname/ULID but may
  not retain payload, receipt, retry history, or durable delivery state.
- Independent CLI/API writes have no cross-command delivery-order guarantee;
  acknowledgement correlation is by immutable ULID.
- Every repeated immutable ULID follows the same write path and remains
  idempotent. No finding authorizes an outbox, replay queue, receipt, retry
  worker, or parallel acknowledgement workflow.
- Product release SemVer is diagnostic only. A separate CLI/daemon schema and
  HTTP API SemVer contract governs admission; same-major additive HTTP changes
  remain interoperable.
- Plaintext smoke diagnosis is selected only by the non-durable daemon CLI
  argument, is visibly labelled, provides untrusted provenance only, and never
  changes the HTTP resource, `WriteRequest`, router, persistence, post-write,
  or production TLS/allowlist evidence contract.
- Each sprint's first commit sets matching workspace CLI/daemon release
  metadata to the current Phase AI prerelease plus its sprint number (for
  example, AI.31 is `1.4.0-beta-ai.31`) and verifies it with `atm doctor
  --json` before runtime evidence.

## Required validation

Each sprint runs `just lint` and `just test`. AI.29 additionally requires
receiver-side evidence for each ULID, recorded CLI/daemon release plus
negotiated schema/HTTP API version, doctor before/after, exactly one daemon per
host, and sanitized logs. Raw TCP success, local persistence, and a sender-side
`sent` event are insufficient evidence.
