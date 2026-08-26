# Sprint AQ1.7 — Graft Endpoint Consumer Cutover (Registry Becomes Truth)

Status: draft · Branch: `feature/aq-1-7-graft-endpoint-cutover` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Third graft connection-model sprint (see AQ1.5). Every production consumer
of the file record switches to the daemon-side registry; after this sprint
the file is written but no longer read by anything in-tree.

## Deliverables

1. **Production wiring (closes critical-review I8)**: `LocalServiceRuntime`
   (`crates/atm-core/src/service_runtime.rs`) gains an optional field, a
   `with_graft_receiver_endpoint_store(store: Arc<dyn
   GraftReceiverEndpointStore + Send + Sync>) -> Self` builder, and a
   `graft_receiver_endpoint_store() -> Result<Arc<dyn
   GraftReceiverEndpointStore + Send + Sync>, AtmError>` accessor —
   mirroring `with_pending_nudge_store`/`async_message_search_store`
   exactly (`service_runtime.rs:200-230`). `RetainedServiceRuntime`
   (`crates/atm-core/src/service_runtime.rs:89`, sealed) gains two new
   methods with `Ok(None)` / `Ok(())` default bodies — mirroring the
   existing `load_nudge_template_override` default-impl precedent — so
   test doubles (`MissingRosterRuntime`, `ListRuntime`) keep compiling
   unchanged: `graft_receiver_lease(team, agent) ->
   Result<Option<GraftReceiverLease>, AtmError>` and
   `mark_graft_receiver_unreachable(team, agent, owner_generation, now) ->
   Result<(), AtmError>`. `impl RetainedServiceRuntime for
   LocalServiceRuntime` overrides both to delegate to the store field when
   present, falling back to the same `Ok(None)`/`Ok(())` a runtime with no
   store attached gives everyone else (correct: no store wired means no
   lease exists, which is honest for a runtime nothing ever registered
   against). `crates/atm-daemon-bootstrap/src/lib.rs`'s
   `assemble_runtime`/`assemble_host_runtime` construct the real rusqlite
   `GraftReceiverEndpointStore` from the same `SqliteStorageFactory`
   already used for `roster_store` (mirroring the `shared_roster_store_arc`
   pattern at `:911`/`:1155`) and call
   `.with_graft_receiver_endpoint_store(store)` on the `LocalServiceRuntime`
   that becomes the real running daemon's `service_runtime` — this is the
   point at which registrations made since AQ1.5/AQ1.6 landed start
   actually persisting/serving against the shipped daemon, closing the gap
   AQ1.6's Non-closure section discloses.
2. **Delivery path (in-daemon; registry becomes the resolution source)**:
   `deliver_published_receiver_hook<R: RetainedServiceRuntime>`
   (`crates/atm-core/src/graft.rs`) drops its `canonical_graft_root` +
   `graft_receiver_record_path_from_root` resolution and instead calls
   deliverable 1's new `graft_receiver_lease` method. A `None` result
   produces the receiver-not-registered error (deliverable 4). `Some(lease)`:
   `deliver_graft_post_send`/`deliver_graft_post_send_with_deadline`
   (`crates/atm-core/src/graft.rs`) are refactored to take the lease's
   endpoint + capability directly instead of `record_path: &Path`, dropping
   their internal `read_receiver_record` call. On connect failure both
   callers call deliverable 1's `mark_graft_receiver_unreachable`
   (staleness is data, never a wedge), then surface today's delivery error
   unchanged — no retry-semantics change, no new error shape.
   **Present-but-expired lease (closes I11; approved by Rand 2026-08-26)**: a lease that exists but
   whose `last_seen_at` is beyond `ACTIVE_LEASE_WINDOW` is dialed anyway —
   delivery does not consult `unreachable_at`/window staleness before
   connecting (mirrors today's file-based behavior, which never checked
   staleness either before dialing). Only an *absent* lease (no row at
   all) is treated as not-registered. Staleness/expiry is display-only
   (doctor, deliverable 4), never a delivery gate.
2b. **Classifier mapping (closes critical review F5)**: the same delivery
   path owns the one place that turns `GraftReceiverEndpointStore::lookup`
   into AQ1's `GraftLeaseState` for `classify_delivery_channel`:
   `fn graft_lease_state(lookup: Option<&GraftReceiverLease>) -> GraftLeaseState`
   in `crates/atm-core/src/delivery_channel.rs`'s consumer module (not the
   classifier itself) — `Some(_)` → `Active`, `None` → `Absent`, regardless
   of `last_seen_at`/`unreachable_at` (expiry is advisory; the dial-anyway
   rule above applies). AQ2/AQ2.5 route `Graft`-classified dispatches
   through this mapping and never re-derive it. AC 8 covers it.
3. **CLI**: `atm _internal-nudge`'s `GraftNudgeSink::deliver`
   (`crates/atm/src/commands/internal_nudge.rs`) drops
   `graft_receiver_record_path_from_home` and instead queries a new daemon
   read route — **recorded here as an AQ1.5 amendment, not yet in AQ1.5's
   Contract (see this plan-finalization pass's report)**:
   `RequestEnvelope::GraftReceiverLookup { team, agent }` /
   `ResponseEnvelope::GraftReceiverLookup(Option<GraftReceiverLease>)`,
   gated `AuthenticatedIngress::Local`, validated against the roster the
   same way register/unregister are — via the same daemon-client seam
   AQ1.6's registration client uses (`atm_daemon_client` +
   `atm_http_runtime::preferred_local_client`; `crates/atm/Cargo.toml`
   already depends on both). The CLI has no local store access and must
   not open the shared SQLite file directly (matches every other
   CLI-daemon-query precedent in this codebase); (team, agent) is the
   entire query key, unlike today's `home_dir`-derived path. `None`
   produces the receiver-not-registered error (deliverable 4); `Some(lease)`
   dials it with deliverable 2's refactored `deliver_graft_post_send`.
4. **Doctor visibility**: `atm doctor --json` gains a graft-receivers
   section (team/agent, endpoint, last_seen age, reachable-at-last-use) —
   read-only over the store via the same `graft_receiver_lease` path as
   delivery (server-side; doctor is served by the daemon's own
   `RuntimeDoctorPorts`, not a second CLI-side query); this is the
   operator's replacement for inspecting record files by hand. Staleness
   is rendered from the read-time aliveness derivation (AQ1.5: lease
   exists AND `last_seen_at` within window AND `unreachable_at IS NULL`) —
   display-only, consistent with deliverable 2's dial-anyway rule (I11).
5. **Fallback removal is explicit**: no silent file fallback remains in any
   cutover consumer — if the lease is absent, the error says the receiver
   is not registered (actionable: receiver not running or daemon missed
   its announce), never a file-read error. (The file keeps being written by
   AQ1.6's dual-write; it is simply unread.)

## Acceptance criteria

1. Delivery integration test: message to a registered receiver delivers
   via the lease endpoint with the file record DELETED beforehand —
   proving the file is no longer load-bearing.
2. Absent-lease delivery and `_internal-nudge` produce the
   receiver-not-registered error naming (team, agent) — no file-path
   errors anywhere (grep gate: `read_receiver_record` and
   `graft_receiver_record_path` have zero call sites outside
   `atm-core/src/graft.rs`'s own write/republish internals — AQ1.6's
   `bind` signature change (deliverable 5) has already migrated the two
   former `crates/atm-graft/src/lib.rs` call sites (~:390 production,
   ~:784 test) off `graft_receiver_record_path_from_root` entirely: they
   now pass `(root, team, agent)` straight into
   `GraftReceiverListener::bind`, which derives both the lock and record
   paths internally, so the only record-path references left are the
   graft.rs internals slated for AQ1.8 deletion; the gate greps the whole
   workspace including atm-graft).
3. Connect-failure path: dead endpoint → `mark_unreachable` recorded +
   today's delivery error surfaced (no new error shapes).
4. Doctor section renders for a live receiver and for a stale lease
   (deterministic fixture).
5. The daemon-restart / receiver-restart matrix (both orders) passes an
   end-to-end test with zero manual steps — the Hermes profile-reset bug
   class is regression-locked here.
6. **Present-but-expired lease still gets dialed, not refused (closes
   I11)**: a lease with `last_seen_at` beyond `ACTIVE_LEASE_WINDOW`
   (simulated clock skew, receiver still actually alive) still receives
   the delivery; a lease belonging to a truly dead process still follows
   the ordinary connect-failure path (AC #3) — expiry alone never triggers
   the receiver-not-registered error.
7. `GraftReceiverLookup` round-trips through `_internal-nudge`'s new query
   path against the real route (integration test, no file record
   involved), mirroring the existing heartbeat handler test style AQ1.5
   uses.

## Required validation

- `cargo test` workspace green on both CI lanes.

## Non-closure / out of scope

- File-record write path and its machinery still exist (deleted in AQ1.8).
- hermes-atm wheel bump (AQ1.9).
- **Version-skew posture**: `atm` CLI and daemon ship and switch as a
  matched release pair in this repo (the daemon-switch tooling enforces
  the pairing), so AC #2's "no file-path errors" claim applies to
  same-version fleets; a pre-AQ1.7 CLI binary against a post-AQ1.8 daemon
  is out of scope and unsupported, like any other unmatched pair.
- **`AlreadyActive` display note**: since AQ1.5's amended `register` no
  longer rejects a same-host, flock-proven re-registration within
  `ACTIVE_LEASE_WINDOW` (critical-review I10, an AQ1.5 amendment recorded
  in this plan-finalization pass's report), doctor's graft-receivers
  section never needs to explain an "already active" registration failure
  to an operator — only `Storage`/network failures reach that path, which
  is already the generic daemon-unavailable presentation.

## Dependencies

- must_follow: AQ1.6 (leases must be populated before consumers depend on
  them); AQ1.5 as amended by this plan-finalization pass for the
  `GraftReceiverLookup` read route deliverable 3 depends on (critical-review
  I9). Merge-forward trigger: AQ1.6 dev push.
- parallel_safe: AQ2.6, AQ2.7 (Herdr — disjoint files; 2026-08-26 reorder).
- Downstream: AQ2 must_follow this sprint (queue-graft channel resolves
  endpoints via the registry; recorded in AQ2's Dependencies).
