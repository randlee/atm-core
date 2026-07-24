---
title: AI.26 receiver-proven Mac-Windows cross-host smoke
status: proposed
branch: feature/pAI-s26-crosshost-smoke-rerun
target: integrate/phase-AI
depends_on: AI.22, AI.23, AI.24, AI.25
---

# AI.26 — receiver-proven Mac↔Windows smoke

## Closure

Mac↔Windows evidence proves each cross-host operation at both sender and
receiver, using the same immutable ULID. Prior sender-only results are
historical diagnostics, not passing evidence.

## Deliverables

1. Amend the shared peer-smoke runner and evidence schema to require a
   receiver-side envelope/ULID record for every successful send or ack.
2. Run hostname and direct-current-IP cases against one hostname-registered
   peer, including DNS-change stale-IP rejection.
3. Execute bidirectional send/read/nudge, requires-ack/ack, duplicate ULID,
   unavailable peer, wrong certificate, allowlist rejection, and failed ack.
4. Record one honest unconfirmed-delivery case and verify no false sent claim,
   receipt, retry state, or sender-side ack mutation is created.
5. Capture one-daemon doctor/listener evidence before and after on both hosts;
   restore the normal installed daemon pair after the smoke.

## Acceptance criteria

- Every positive case has matching sender request and receiver persisted ULID.
- Sender local persistence or raw TCP alone fails the evidence validator.
- Negative cases fail before receiver mailbox mutation and expose the expected
  typed error/event.
- Both hosts use one matching CLI/daemon pair throughout and restore their
  normal pair afterward.

## Required validation

`just lint` and `just test` at the exact tested commit on both hosts; complete
sanitized evidence bundle; runner schema validation; quality review.

## Non-closure

No production feature beyond the AI.22–AI.25 fixes is added here.
