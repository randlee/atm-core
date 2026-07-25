---
title: AI.28 bounded peer recovery after connectivity loss
status: proposed
branch: feature/pAI-s28-bounded-peer-recovery
target: integrate/phase-AI
depends_on: AI.26, AI.27
---

# AI.28 — bounded peer recovery after connectivity loss

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta-28`; record
  matching client/daemon values from `atm doctor --json` in runtime evidence.

## Closure

After a peer-connectivity failure, the one daemon re-attempts ordinary bounded
reconciliation only while recent immutable outbound records remain inside the
configured send window. It creates no message retry queue or delivery receipt.

## Deliverables

1. Extend the existing durable `PeerSyncPolicy` with an operator-selected send
   window and bounded recovery cadence. The initial smoke policy is 10 minutes
   and batch cap 100.
2. Add one daemon-owned bounded in-memory per-peer schedule: first retry no
   earlier than 60 seconds; later failures exponential-backoff to a 15-minute
   cap; daemon restart also waits at least 60 seconds.
3. At each due time query the storage trait for immutable outbound records in
   the window and submit each through the existing HTTPS adapter. No queue,
   cursor, checkpoint, payload cache, receipt, or per-message retry state.
4. Emit retained `peer_recovery_scheduled`, `peer_recovery_attempt`,
   `peer_recovery_confirmed`, and `peer_recovery_unconfirmed` events with peer,
   bounded candidate count, delay, error code, and ULID where applicable.
5. Stop scheduling when the window is empty, policy disabled, or peer revoked.
   `atm peer sync` remains one immediate bounded pass with the same events.

## Acceptance criteria

- No network retry starts sooner than one minute after Wi-Fi/VPN loss.
- Backoff grows after failures, caps at 15 minutes, and resets only after peer
  HTTP acceptance.
- Recovery reuses original ULIDs and receiver duplicates remain idempotent.
- Events never call local persistence a recovery success and expose no body or
  certificate material.
- Exactly one scheduler and HTTPS delivery call path exist; transport imports
  no SQLite type.

## Required validation

Fake-clock tests for minimum/cap/reset/revoke/empty-window/restart; integration
test with original ULID; event-schema tests; `just lint`; `just test`.

## Non-closure

No heartbeat/ping protocol, TCP probe loop, alternate transport, remote
delivery table, or separate write path is added.
