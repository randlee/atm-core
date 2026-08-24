# Sprint AQ1.5 — Graft Receiver Registration: Daemon API + Durable Store

Status: draft · Branch: `feature/aq-1-5-graft-registration-api` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

First of the graft connection-model sprints (AQ1.5–AQ1.9) inserted per Rand
2026-08-24: replace the file-based receiver endpoint record with
push-registration to the daemon, making the SQLite-backed runtime the single
source of truth for receiver endpoints. Motivation: the record file's
truncating-write race (open finding AI3133 defect #2) causes live Hermes
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
   on `RosterStore`), with a rusqlite implementation and idempotent
   `CREATE TABLE IF NOT EXISTS graft_receiver_endpoints` +
   `ensure_graft_receiver_endpoint_columns` migration helper in
   `crates/atm-storage-rusqlite/src/shared_db.rs` (the repo's established
   inline-schema pattern — there is no migration framework).
3. **Handler**: registration/unregistration in
   `crates/atm-http-runtime/src/storage_and_nudge_router.rs`, mirroring
   `heartbeat`/`validate_heartbeat_member`: gated to
   `AuthenticatedIngress::Local` only, member validated against the roster.
   Unlike heartbeat's in-memory `RuntimeHealth` (which deliberately resets
   on daemon restart), writes go to the durable store.
4. **Displacement rule (AI3133 property (c), decided here)**: a register
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
   planned supersession of AI3133 (defects #1/#3 already fixed by AI.36's
   flock + generation-checked Drop; #2 is eliminated with the file itself
   in AQ1.8).

## Contract (normative)

```rust
pub struct GraftReceiverRegistration {
    pub team: TeamName,
    pub agent: AgentName,
    pub endpoint: SocketAddr,          // loopback only; validated
    pub capability: LocalCapability,   // per-bind token, as today
    pub owner_generation: String,      // ULID, as today
}

// graft_receiver_endpoints table columns:
// team, agent, endpoint, capability, owner_generation,
// registered_at (UTC), last_seen_at (UTC)
// PRIMARY KEY (team, agent)

pub trait GraftReceiverEndpointStore: Send + Sync {
    /// Idempotent lease upsert. Err(AlreadyActive) iff an existing row has
    /// a different owner_generation and last_seen_at within the active
    /// window (see ADR-056; window = 3 × refresh interval).
    fn register(&self, reg: &GraftReceiverRegistration, now: DateTime<Utc>)
        -> Result<(), GraftEndpointStoreError>;
    /// Refresh last_seen_at; Err(NotOwner) on generation mismatch.
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

## Acceptance criteria

1. Round-trip unit tests: register → lookup → refresh → unregister against
   the rusqlite store; upsert idempotence (same generation re-register
   refreshes, never errors).
2. Displacement truth table: live lease + different generation → rejected
   with already-active error; expired lease (last_seen beyond window) +
   different generation → replaced; matching generation → refresh.
3. Persistence across restart: store reopened from the same DB file
   returns the lease (daemon-restart survival — the lifecycle
   requirement's storage half).
4. Handler tests: non-local ingress rejected; unknown roster member
   rejected; envelope round-trips through the route spec (mirroring the
   existing heartbeat handler tests).
5. `ensure_*` migration helper is idempotent on an existing DB (test opens
   a pre-migration fixture DB twice).
6. `cargo test` workspace + boundary-enforcement suite green on both CI
   lanes; ADR-056 present and indexed.

## Non-closure / out of scope

- Receiver-side registration calls (AQ1.6), consumer cutover (AQ1.7),
  file-record deletion (AQ1.8).
- Any change to the existing file-record code paths — they continue to
  work unchanged this sprint (dual-write comes in AQ1.6).

## Dependencies

- must_follow: AQ1 (shared files: `protocol.rs`, router, storage traits are
  also in AQ1's touch set). Merge-forward trigger: AQ1 dev push.
- parallel_safe: none claimed.
- AQ2 must_follow AQ1.7 (recorded in AQ2's own Dependencies section, which
  is authoritative).
