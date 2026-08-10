---
title: AP.5 — Operations and real CWin proof
status: planned
recommended_agent: Cipher-311d
---

# AP.5 — Operations and real CWin proof

## Scope

Add safe visibility and control around the merged AP.4 live-session transport,
then prove it on the real CWin↔M4 path originally demonstrated by AP.1.

## Dependencies

- **must_follow:** AP.4 PR merged.
- **parallel_safe:** none. This is the Phase AP release proof.
- **unblocks:** Phase AP closure.

## Deliverables

1. Doctor/status projection: configured mode, authenticated/live state, peer
   hostname, generation, last transition, and bounded in-flight count; no
   private material or message bodies.
2. Explicit enable, disable, and status controls. Disable stops new work,
   drains only to the bounded deadline, and removes the registry entry.
3. Real CWin↔M4 evidence: CWin outbound session, M4-originated canonical
   send, CWin persistence/nudge, correlated result, requires-ack/reply,
   disconnect/reconnect, disabled peer, wrong certificate, saturated capacity,
   and no-live-session failure.
4. Indexed safe reports under `site/reports/` following the smoke-test
   convention.

## Acceptance criteria

- Every physical and negative result is captured at one candidate SHA.
- Normal local/direct peer regression stays green.
- The proof relies on neither a tunnel nor an external relay.
- Doctor/control output never leaks keys, tokens, or message bodies.

## Required validation

- Unit/integration tests for doctor and control transitions.
- Full AP.1–AP.4 regression plus local/direct-peer smoke.
- Real CWin report panel and master-index verification.
- `just lint` and `just test` at the frozen candidate.

## Non-closure

AP.5 does not add offline delivery, retry, replay, or an external relay.
