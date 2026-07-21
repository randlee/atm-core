---
title: AI.16 bounded offline peer reconciliation
status: proposed
branch: feature/pAI-s16-offline-reconciliation
worktree: ../atm-core-worktrees/feature/pAI-s16-offline-reconciliation
target: integrate/phase-AI
depends_on: AI.12, AI.13, AI.14, AI.15
---

# AI.16 — bounded offline peer reconciliation

## Closure

When a peer becomes reachable, the daemon can re-send recent immutable local
outbound messages to that peer from canonical storage. The feature is bounded
by a durable user setting and uses existing ULID idempotency; it creates no
outbox, replay store, queue, receipt, checkpoint, or per-message delivery state.

## Deliverables

1. Implement ADR-038, `REQ-CORE-TRANSPORT-003A`, and
   `REQ-DAEMON-TRANSPORT-002D`: the bounded reconciliation scan is a storage
   query over canonical immutable messages while delivery-state subsystems
   remain prohibited.
2. Add a backend-neutral durable `PeerSyncPolicy` through `PeerConfigStore`:

```rust
pub struct PeerSyncPolicy {
    pub max_message_age: Duration,
}

pub trait PeerConfigStore {
    fn peer_sync_policy(&self) -> Result<PeerSyncPolicy, AtmError>;
    fn set_peer_sync_policy(&self, policy: PeerSyncPolicy) -> Result<(), AtmError>;
}
```

   The initial default is disabled (`max_message_age = 0`) until an operator
   explicitly enables it through CLI. It is never environment-driven.
3. Add operator commands to show/set the maximum reconciliation age and to
   request a one-shot peer sync. The command validates a known trusted peer and
   emits ordinary progress/error output; it does not create a persistent job.
4. After a successful ordinary peer request, and for an explicit one-shot sync,
   query canonical local outbound records whose destination host is that peer
   and whose creation time is within `max_message_age`. Re-send each exact
   stored `WriteRequest` ULID/payload through `PeerHttpTransport`.
5. Never scan recipient inbox records, mutate stored messages, create a sync
   cursor, or suppress/rewrite a conflict. Exact duplicate arrival is storage
   idempotent; a same-ULID/different-payload conflict follows AI.12's typed
   error/log/no-side-effect rule.

## Boundary contract

```rust
pub trait OutboundMessageQuery: Send + Sync {
    fn recent_outbound_for_peer(
        &self,
        peer: &HostName,
        not_before: IsoTimestamp,
    ) -> Result<Vec<WriteRequest>, AtmError>;
}
```

Only the selected storage backend implements the query. The reconciliation
coordinator may choose a peer and invoke `PeerHttpTransport`; it may not use
SQLite types, inspect schema, own a worker queue, or decide normal write
routing. It has no connection-health state: every successful ordinary peer
response may trigger the bounded scan, while the explicit sync command provides
an operator trigger when no new write exists.

## Acceptance criteria

- With policy disabled, no automatic or explicit reconciliation sends occur.
- With policy enabled, only recent local outbound records for the selected peer
  are re-sent with their exact original ULIDs and immutable payloads.
- A reconnect/success followed by reconciliation delivers messages missed while
  offline; duplicate receiver arrival creates no second row/nudge/ack mutation.
- Older records, other peers, and local-recipient records are excluded.
- Failure returns typed errors/logs and leaves no persistent delivery state.
- CLI, daemon, and transport access configuration/storage only through traits;
  no environment setting or direct SQLite access is added.

## Required tests

| Level | Proof |
| --- | --- |
| Storage | age/peer/direction filtering; exact stored payload/ULID returned; backend-neutral contract tests. |
| Unit | disabled policy; successful-request trigger; explicit-sync trigger; no cursor/queue mutation. |
| Integration | offline peer then available peer delivers recent message; duplicate arrival is one record/one nudge; stale and wrong-peer records excluded. |
| Negative | payload conflict logs/returns typed error with no side effect; untrusted peer is rejected before scan delivery. |
| Smoke | Extend the AI.13 runner and execute offline→online reconciliation with an enabled bounded policy on the already-proven Mac↔Mac and Mac↔Windows pairs; record evidence. |

## Required validation

Run every storage/unit/integration/negative/smoke proof above, then run the
storage-boundary and transport-state architecture gates, `just lint`, and
`just test`.

## Non-closure

AI.16 does not implement continuous reconnect monitoring, background polling,
retry budgets, delivery receipts, or a durable outbox. Operators select the
maximum age explicitly; zero disables the feature.
