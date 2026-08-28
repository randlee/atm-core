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

1. **Wire contract**: `RequestEnvelope::GraftReceiverRegister`,
   `::GraftReceiverUnregister`, and `::GraftReceiverLookup { team, agent }`
   (+ matching `ResponseEnvelope` variants, including
   `GraftReceiverLookup(Option<GraftReceiverLease>)`) in
   `crates/atm-core/src/protocol.rs`, and an `HttpRouteKind` + route spec
   for each in `crates/atm-core/src/api.rs`, modeled on the existing
   `Heartbeat` route. Register is an idempotent lease upsert (re-announce =
   refresh). `GraftReceiverLookup` is a read route, gated
   `AuthenticatedIngress::Local`-only like register/unregister (critical
   review I9); AQ1.7's `_internal-nudge` delivery path and `atm doctor`
   are its consumers.
2. **Durable store**: `GraftReceiverEndpointStore` trait in
   `crates/atm-storage/src/contract.rs` (sealed, `Send + Sync`, patterned
   on `RosterStore`, including the `sealed::Sealed` bound every sibling
   storage trait carries — plus `impl sealed::Sealed for` the rusqlite
   implementation), with a rusqlite implementation and idempotent
   `CREATE TABLE IF NOT EXISTS graft_receiver_endpoints` +
   `ensure_graft_receiver_endpoint_columns` migration helper in
   `crates/atm-storage-rusqlite/src/shared_db.rs` (the repo's established
   inline-schema pattern — there is no migration framework).
3. **Handler**: registration/unregistration/lookup in
   `crates/atm-http-runtime/src/storage_and_nudge_router.rs`, mirroring
   `heartbeat`/`validate_heartbeat_member`: gated to
   `AuthenticatedIngress::Local` only, member validated against the roster.
   Unlike heartbeat's in-memory `RuntimeHealth` (which deliberately resets
   on daemon restart), writes go to the durable store. **The handler
   rejects any non-loopback `endpoint` value** — ingress gating restricts
   who may call; this validates what they submit. (Delivery must never be
   induced to dial a non-loopback address carrying the capability token.)
   The lookup handler is read-only (I9): same ingress/roster gating as
   register/unregister, returning `GraftReceiverEndpointStore::lookup`'s
   result verbatim — a miss is `Ok(None)`, not an error.
4. **Displacement rule (AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE
   property (c); revised 2026-08-26, critical review I10; approved by Rand 2026-08-26)**: `register` is
   `AuthenticatedIngress::Local`-only, and its sole in-tree caller
   (`GraftReceiverListener::bind`'s post-flock registration, now in
   `atm-graft`) has already OS-proven same-host exclusivity via the flock
   before it ever calls `register`. `register` therefore unconditionally
   displaces the stored lease on `owner_generation` mismatch — no
   `ACTIVE_LEASE_WINDOW`-gated rejection; the flock, not the lease table,
   is the exclusivity mechanism. The `AlreadyActive` error variant is
   retained on the trait for a future caller that cannot prove same-host
   exclusivity by construction (none exists today); `register` itself
   never returns it.
5. **ADR-056 — graft receiver registration and lease semantics**
   (`docs/adr/ADR-056-graft-receiver-registration-and-lease.md`, INDEX.md
   entry): records the push-registration decision, the lease/refresh model,
   the displacement rule, the lifecycle-independence requirement, and the
   planned supersession of AI3133-HERMES-GRAFT-RECEIVER-SINGLETON-UNSAFE (defects #1/#3 already fixed by AI.36's
   flock + generation-checked Drop; #2 is eliminated with the file itself
   in AQ1.8).
6. **`LocalServiceRuntime` wiring, test-only (I8)**: a
   `with_graft_receiver_endpoint_store(self, store: Arc<dyn
   GraftReceiverEndpointStore>) -> Self` builder and a
   `graft_receiver_endpoint_store() -> Result<Arc<dyn
   GraftReceiverEndpointStore>, AtmError>` accessor on
   `LocalServiceRuntime`, mirroring `with_pending_nudge_store` /
   `pending_nudge_store()` (see `aq1-blueprint.md`); and two default
   methods on `RetainedServiceRuntime` — `Ok(None)` for the lookup default
   and `Ok(())` for the not-configured write default — precedent
   `load_nudge_template_override`. Wired here for this sprint's own tests
   only; AQ1.7 owns the production bootstrap wiring that registers the
   real store instance (see Dependencies).

## Contract (normative)

```rust
/// Every trait method returns this; callers' obligations are fixed here.
pub enum GraftEndpointStoreError {
    /// Reserved for a future caller that cannot prove same-host
    /// exclusivity by construction. `register`'s sole in-tree caller
    /// (post-flock `GraftReceiverListener::bind`) already has OS-proven
    /// same-host exclusivity, so `register` unconditionally displaces on
    /// owner_generation mismatch and never returns this today (revised
    /// 2026-08-26, critical review I10).
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

// capability encoding (I12): wire- and at-rest-encoded via
// LocalCapability::to_base64url() / parse_base64url() — same as today's
// capability_base64url. The file record's 0o600 permission guarantee
// retires with the file (AQ1.8); DB-at-rest confidentiality for this
// column is parity with roster/message data already in this SQLite file
// — no new hardening is introduced in this phase.

pub trait GraftReceiverEndpointStore: sealed::Sealed + Send + Sync {
    /// Idempotent lease upsert (also clears unreachable_at).
    /// Unconditional displacement on owner_generation mismatch (revised
    /// 2026-08-26, critical review I10) — the sole in-tree caller already
    /// has OS-proven same-host exclusivity via flock before calling this.
    /// Never returns AlreadyActive today; reserved on the trait for a
    /// future non-flock-provable caller.
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
- this read-time aliveness predicate is unrelated to `register`'s
  write-time exclusivity check (deliverable 4; revised 2026-08-26,
  critical review I10, now flock-derived rather than a window check) —
  that check never consults `unreachable_at`; unreachable is
  delivery-advisory display state, not a write-conflict input. Two
  predicates, two purposes, both defined here.

Lease timing constants (defined here/ADR-056, implemented by AQ1.6):
`GRAFT_LEASE_REFRESH_INTERVAL = 1s` (matches the existing
`GRAFT_RECEIVER_RECORD_RECHECK_INTERVAL` cadence) and
`ACTIVE_LEASE_WINDOW = 15 × GRAFT_LEASE_REFRESH_INTERVAL = 15s`. The wide
multiple is deliberate headroom against refresh jitter and momentary
daemon/storage hiccups — a live receiver missing a handful of ticks must
never look displaceable (see AQ1.6's every-iteration refresh rule).

**Amendment (2026-08-26, AQ1 classifier integration, D7):** AQ1 ships
`enum GraftLeaseState { Absent, Active }` as the graft input to its
`classify_delivery_channel` classifier, replacing the earlier
`Option<&GraftReceiverLease>` sketch to avoid an AQ1↔AQ1.5 type-name
collision (AQ1 compiles before this sprint's `GraftReceiverLease` exists).
AQ1.7's consumer-cutover call site is the mapping point: it calls
`GraftReceiverEndpointStore::lookup` and maps the result to
`GraftLeaseState::Active` **iff a lease row exists for the member** (any
`last_seen_at`/`unreachable_at` — expiry is advisory and AQ1.7 dials
expired leases anyway, I11; the read-side liveness derivation above is for
doctor/health reporting, not channel selection) — else `Absent`. The
mapping deliverable and its AC live in AQ1.7 (deliverable 2).

## Acceptance criteria

1. Round-trip unit tests: register → lookup → refresh → unregister against
   the rusqlite store; upsert idempotence (same generation re-register
   refreshes, never errors); mark_unreachable sets `unreachable_at` and a
   subsequent refresh or register clears it.
2. Displacement truth table (revised 2026-08-26, critical review I10):
   `register` with a different generation unconditionally displaces
   (replaces) the existing lease, live or expired — flock already proved
   same-host exclusivity before the call; matching generation → refresh.
   No rejection path exists for `register` in this sprint's tests; the
   retained `AlreadyActive` variant has no in-tree exerciser today.
3. Persistence across restart: store reopened from the same DB file
   returns the lease (daemon-restart survival — the lifecycle
   requirement's storage half).
4. Handler tests: non-local ingress rejected; unknown roster member
   rejected; non-loopback `endpoint` in a register request rejected; envelope round-trips through the route spec (mirroring the
   existing heartbeat handler tests).
5. `ensure_*` migration helper is idempotent on an existing DB (test opens
   a pre-migration fixture DB twice).
6. `GraftReceiverLookup` route round-trips (I9): a registered lease is
   returned; an unknown (team, agent) returns `Ok(None)`, not an error;
   non-local ingress is rejected with the same gating as
   register/unregister.
7. `LocalServiceRuntime` test wiring (I8):
   `with_graft_receiver_endpoint_store` / `graft_receiver_endpoint_store()`
   round-trip against a fixture store in this sprint's own tests;
   `RetainedServiceRuntime`'s default `Ok(None)`/`Ok(())` methods are
   exercised where no store is wired.

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
- Downstream (I8 split): this sprint wires
  `with_graft_receiver_endpoint_store` / `graft_receiver_endpoint_store()`
  on `LocalServiceRuntime` and the `RetainedServiceRuntime` defaults for
  its own tests only. AQ1.7 owns registering the real store instance in
  production daemon bootstrap — this sprint's test wiring is not that
  wiring.
