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
    pub max_batch_messages: NonZeroU16,
}

pub trait PeerConfigStore {
    fn peer_sync_policy(&self) -> Result<PeerSyncPolicy, AtmError>;
    fn set_peer_sync_policy(&self, policy: PeerSyncPolicy) -> Result<(), AtmError>;
}
```

   The initial default is disabled (`max_message_age = 0`) until an operator
   explicitly enables it through CLI. `max_batch_messages` defaults to `100`
   and is a hard upper bound. It is never environment-driven.
3. Add these exact additions-only CLI forms; `<peer>` is an exact configured
   trusted host identity and `<age>` is a positive whole-second duration:

```text
atm peer sync-policy show <peer>
atm peer sync-policy set <peer> --max-message-age <age>
atm peer sync <peer>
```

   `set ... --max-message-age 0s` disables automatic and explicit sync for that
   peer. `sync` is synchronous, returns the ordinary structured error on an
   unknown/untrusted peer or failed transport, and never creates a persistent
   job.
4. After a successful ordinary peer request, and for an explicit one-shot sync,
   query canonical local outbound records whose destination host is that peer
   and whose creation time is within `max_message_age`. Re-send each exact
   stored `WriteRequest` ULID/payload through `PeerHttpTransport`.
5. Never scan recipient inbox records, mutate stored messages, create a sync
   cursor, or suppress/rewrite a conflict. Exact duplicate arrival is storage
   idempotent; a same-ULID/different-payload conflict follows AI.12's typed
   error/log/no-side-effect rule.
6. Add/update `boundaries/atm-storage/peer-config-store.toml` for
   `PeerConfigStore` and add `boundaries/atm-storage/outbound-message-query.toml`
   for `OutboundMessageQuery`. Both boundary records allow only
   backend-neutral domain types and reject SQLite, transport, nudge, or router
   dependencies.

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
routing. It has no connection-health state. It selects at most
`max_batch_messages`, ordered oldest-first by creation time. Automatic sync may
run at most once per peer per 60 seconds using a bounded in-memory
`HostName -> Instant` cooldown map containing no message IDs or payloads; it
does not retry, back off, schedule work, or survive daemon restart. Explicit
`atm peer sync` remains an immediate one-batch operator trigger.

## Acceptance criteria

- With policy disabled, no automatic or explicit reconciliation sends occur.
- With policy enabled, only recent local outbound records for the selected peer
  are re-sent with their exact original ULIDs and immutable payloads.
- A reconnect/success followed by reconciliation delivers messages missed while
  offline; duplicate receiver arrival creates no second row/nudge/ack mutation.
- Older records, other peers, and local-recipient records are excluded.
- One trigger sends no more than `max_batch_messages`; repeated successful
  writes inside the 60-second automatic cooldown do not start another scan.
- Failure returns typed errors/logs and leaves no persistent delivery state.
- CLI, daemon, and transport access configuration/storage only through traits;
  no environment setting or direct SQLite access is added.

## Required tests

| Level | Proof |
| --- | --- |
| Storage | age/peer/direction filtering; exact stored payload/ULID returned; backend-neutral contract tests. |
| Unit | disabled policy; successful-request trigger; explicit-sync trigger; 100-message batch cap; 60-second cooldown; no cursor/queue mutation. |
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
