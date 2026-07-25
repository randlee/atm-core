---
title: AI.29 receiver-proven Mac-Windows cross-host smoke
status: proposed
branch: feature/pAI-s29-crosshost-smoke-rerun
target: integrate/phase-AI
depends_on: AI.25, AI.26, AI.27, AI.28, AI.30
---

# AI.29 — receiver-proven Mac↔Windows smoke

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta-29`; record
  matching client/daemon values from `atm doctor --json` in runtime evidence.

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
- Each host records its CLI and daemon release plus negotiated schema and HTTP
  API version; release strings may differ only when the compatibility contract
  succeeds. Both restore their normal pair afterward.

## Required validation

`just lint` and `just test` at the exact tested commit on both hosts; complete
sanitized evidence bundle; runner schema validation; quality review.

## Non-closure

No production feature beyond the AI.25–AI.28 smoke fixes and AI.30
compatibility contract is added here.
