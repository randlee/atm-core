# Sprint AQ1.5 — Graft Receiver Registration: Daemon API + Durable Store

Status: draft · Branch: `feature/aq-1-5-graft-registration-api` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

First of the graft connection-model sprints (AQ1.5–AQ1.9) inserted per Rand
2026-08-24: replace the file-based receiver endpoint record with
push-registration to the daemon, making the SQLite-backed runtime the single
source of truth for receiver endpoints. Motivation: the record file's
truncating-write race (open finding AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE defect #2) causes live Hermes
agents to fall back to CLI/file workarounds, and AQ2's queue-graft channel
must not ship on that foundation.

**Binding lifecycle requirement (Rand, 2026-08-24):** the daemon and graft
receivers have independent lifecycles. Neither restarting may ever require
manual intervention (e.g. a Hermes profile reset) to restore delivery.
Registrations therefore persist in SQLite (they survive daemon restarts);
receivers refresh their lease on a timer; liveness is validated at use.

## Deliverables

1. **Wire contract**: `RequestEnvelope::GraftReceiverRegister` and
   `::GraftReceiverUnregister` (+ matching `ResponseEnvelope` variants) in
   `crates/atm-core/src/protocol.rs`, and a `HttpRouteKind` + route spec in
   `crates/atm-core/src/api.rs`, modeled on the existing `Heartbeat`
   route. Register is an idempotent lease upsert (re-announce = refresh).
2. **Durable store**: `GraftReceiverEndpointStore` trait in
   `crates/atm-storage/src/contract.rs` (sealed, `Send + Sync`, patterned
   on `RosterStore`, including the `sealed::Sealed` bound every sibling
   storage trait carries — plus `impl sealed::Sealed for` the rusqlite
   implementation), with a rusqlite implementation and idempotent
   `CREATE TABLE IF NOT EXISTS graft_receiver_endpoints` +
   `ensure_graft_receiver_endpoint_columns` migration helper in
   `crates/atm-storage-rusqlite/src/shared_db.rs` (the repo's established
   inline-schema pattern — there is no migration framework).
3. **Handler**: registration/unregistration in
   `crates/atm-http-runtime/src/storage_and_nudge_router.rs`, mirroring
   `heartbeat`/`validate_heartbeat_member`: gated to
   `AuthenticatedIngress::Local` only, member validated against the roster.
   Unlike heartbeat's in-memory `RuntimeHealth` (which deliberately resets
   on daemon restart), writes go to the durable store. **The handler
   rejects any non-loopback `endpoint` value** — ingress gating restricts
   who may call; this validates what they submit. (Delivery must never be
   induced to dial a non-loopback address carrying the capability token.)
4. **Displacement rule (AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE property (c), decided here)**: a register
   for a (team, agent) whose existing lease has a different
   `owner_generation` AND `last_seen_at` within the active-lease window is
   **rejected** with the graft-receiver-already-active error (mirroring
   today's `republish_if_missing` refusal); a stale or unregistered lease
   is replaced atomically (single upsert under SQLite's write
   serialization). Displacement is therefore impossible silently — a live
   receiver is never unseated, and the second registrant gets an explicit
   error.
5. **ADR-056 — graft receiver registration and lease semantics**
   (`docs/adr/ADR-056-graft-receiver-registration-and-lease.md`, INDEX.md
   entry): records the push-registration decision, the lease/refresh model,
   the displacement rule, the lifecycle-independence requirement, and the
   planned supersession of AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE (defects #1/#3 already fixed by AI.36's
   flock + generation-checked Drop; #2 is eliminated with the file itself
   in AQ1.8).

## Contract (normative)

```rust
/// Every trait method returns this; callers' obligations are fixed here.
pub enum GraftEndpointStoreError {
    /// register: an existing lease with a different owner_generation is
    /// still within the active window. AQ1.6 client: log + back off; the
    /// bind itself still succeeds (flock already proved same-host
    /// exclusivity; cross-host duplicates surface here).
    AlreadyActive,
    /// refresh/unregister: caller's owner_generation does not match the
    /// stored lease. AQ1.6 client: stop refreshing this generation
    /// (a newer bind owns the lease); never retry.
    NotOwner,
    /// lookup miss is NOT an error (Ok(None)); this variant is for
    /// underlying SQLite/I-O failures on any method. AQ1.6 client: treat
    /// like daemon-unavailable (retry next tick); AQ1.7 consumers:
    /// surface as today's delivery infrastructure error.
    Storage(String),
}

pub struct GraftReceiverLease {
    pub endpoint: SocketAddr,
    pub capability: LocalCapability,
    pub owner_generation: String,
    pub registered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    /// Set by mark_unreachable; cleared by the next successful refresh or
    /// re-register. AQ1.7 doctor renders it as reachable-at-last-use;
    /// delivery treats it as advisory only (it still attempts the dial).
    pub unreachable_since: Option<DateTime<Utc>>,
}

// Envelope shapes (protocol.rs): GraftReceiverRegister carries the full
// GraftReceiverRegistration; GraftReceiverUnregister carries only
// { team, agent, owner_generation }, mirroring the store's unregister.

pub struct GraftReceiverRegistration {
    pub team: TeamName,
    pub agent: AgentName,
    pub endpoint: SocketAddr,          // loopback only; validated
    pub capability: LocalCapability,   // per-bind token, as today
    pub owner_generation: String,      // ULID, as today
}

// graft_receiver_endpoints table columns:
// team, agent, endpoint, capability, owner_generation,
// registered_at (UTC), last_seen_at (UTC),
// unreachable_at (UTC, NULLABLE) — backs mark_unreachable/
//   GraftReceiverLease.unreachable_since; CLEARED (set NULL) by any
//   successful refresh or register for the same (team, agent)
// PRIMARY KEY (team, agent)

pub trait GraftReceiverEndpointStore: sealed::Sealed + Send + Sync {
    /// Idempotent lease upsert (also clears unreachable_at).
    /// Err(AlreadyActive) iff an existing row has a different
    /// owner_generation and last_seen_at within ACTIVE_LEASE_WINDOW.
    fn register(&self, reg: &GraftReceiverRegistration, now: DateTime<Utc>)
        -> Result<(), GraftEndpointStoreError>;
    /// Refresh last_seen_at and clear unreachable_at;
    /// Err(NotOwner) on generation mismatch.
    fn refresh(&self, team: &TeamName, agent: &AgentName,
        owner_generation: &str, now: DateTime<Utc>) -> Result<(), GraftEndpointStoreError>;
    /// Remove iff owner_generation matches (mirrors generation-checked Drop).
    fn unregister(&self, team: &TeamName, agent: &AgentName,
        owner_generation: &str) -> Result<(), GraftEndpointStoreError>;
    fn lookup(&self, team: &TeamName, agent: &AgentName)
        -> Result<Option<GraftReceiverLease>, GraftEndpointStoreError>;
    /// Delivery-time staleness feedback: connect failure marks the lease
    /// suspect without deleting it (see AQ1.7 for consumer semantics).
    fn mark_unreachable(&self, team: &TeamName, agent: &AgentName,
        owner_generation: &str, now: DateTime<Utc>) -> Result<(), GraftEndpointStoreError>;
}
```

**Read-side consumers and liveness derivation (Rand, 2026-08-24; recorded
in ADR-056):** the lease table's primary key (team, agent) is the same
natural key as `team_roster`; consumers (roster views, the planned
liveness hooks, doctor) read via `LEFT JOIN ... USING (team, agent)` — no
SQL `FOREIGN KEY` is declared. The schema does have FK precedent
(`mail_message_states` → `mail_messages`, ON DELETE CASCADE), but it does
not apply here: message-state rows are written 1:1 alongside their parent
message and share its lifecycle, whereas `save_roster` does a per-team
`DELETE FROM team_roster` + bulk re-INSERT on every save — a real FK from
the lease table to roster would either cascade-drop live leases on every
roster save or reject the save. The shared natural key expresses the
relationship; the constraint would only fight the roster write pattern.
Anti-state-machine guardrails, binding on every consumer:

- exactly two writers, ever: the receiver (register/refresh/unregister)
  and the delivery path (`mark_unreachable`); everything else is
  read-only;
- aliveness is DERIVED at read time — `lease exists AND last_seen_at
  within ACTIVE_LEASE_WINDOW AND unreachable_at IS NULL` — never stored
  as a boolean/status column, so there is no transition to miss and
  nothing to fall out of sync;
- no mirroring into roster rows, no events/subscriptions/callbacks at
  this layer — a future liveness-hook feature builds on these rows,
  reading the same derivation;
- this read-time aliveness predicate is intentionally STRICTER than the
  displacement rule's active-lease-window check (deliverable 4), which
  never consults `unreachable_at` — unreachable is delivery-advisory
  display state, not a write-conflict input. Two predicates, two
  purposes, both defined here.

Lease timing constants (defined here/ADR-056, implemented by AQ1.6):
`GRAFT_LEASE_REFRESH_INTERVAL = 1s` (matches the existing
`GRAFT_RECEIVER_RECORD_RECHECK_INTERVAL` cadence) and
`ACTIVE_LEASE_WINDOW = 15 × GRAFT_LEASE_REFRESH_INTERVAL = 15s`. The wide
multiple is deliberate headroom against refresh jitter and momentary
daemon/storage hiccups — a live receiver missing a handful of ticks must
never look displaceable (see AQ1.6's every-iteration refresh rule).

## Acceptance criteria

1. Round-trip unit tests: register → lookup → refresh → unregister against
   the rusqlite store; upsert idempotence (same generation re-register
   refreshes, never errors); mark_unreachable sets `unreachable_at` and a
   subsequent refresh or register clears it.
2. Displacement truth table: live lease + different generation → rejected
   with already-active error; expired lease (last_seen beyond window) +
   different generation → replaced; matching generation → refresh.
3. Persistence across restart: store reopened from the same DB file
   returns the lease (daemon-restart survival — the lifecycle
   requirement's storage half).
4. Handler tests: non-local ingress rejected; unknown roster member
   rejected; non-loopback `endpoint` in a register request rejected; envelope round-trips through the route spec (mirroring the
   existing heartbeat handler tests).
5. `ensure_*` migration helper is idempotent on an existing DB (test opens
   a pre-migration fixture DB twice).

## Required validation

- `cargo test` workspace + boundary-enforcement suite green on both CI
  lanes; ADR-056 present and indexed.

## Non-closure / out of scope

- Receiver-side registration calls (AQ1.6), consumer cutover (AQ1.7),
  file-record deletion (AQ1.8).
- Any change to the existing file-record code paths — they continue to
  work unchanged this sprint (dual-write comes in AQ1.6).

## Dependencies

- must_follow: AQ1 (shared files: `protocol.rs`, router, storage traits are
  also in AQ1's touch set). Merge-forward trigger: AQ1 dev push.
- parallel_safe: AQ2.6, AQ2.7 (Herdr — disjoint files; 2026-08-26 reorder).
- AQ2 must_follow AQ1.7 (recorded in AQ2's own Dependencies section, which
  is authoritative).
