# Sprint AQ1.6 — Graft Receiver Registration Client (Announce-at-Init)

Status: draft · Branch: `feature/aq-1-6-graft-registration-client` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Second graft connection-model sprint (see AQ1.5 for motivation and the
binding lifecycle requirement). The receiver announces its endpoint to the
daemon at bind time and keeps its lease fresh; the file record is still
written (dual-write) so nothing downstream changes until AQ1.7's cutover.

## Deliverables

1. **Announce at bind**: `GraftReceiverListener::bind`
   (`crates/atm-core/src/graft.rs`) — after binding `127.0.0.1:0`,
   acquiring the existing `ReceiverOwnershipGuard` flock (which stays: it
   remains the same-host mutual-exclusion primitive), and generating the
   `LocalCapability` + `owner_generation` exactly as today — registers with
   the daemon via the same client seam `GraftClient` already uses for
   send/read (`atm_daemon_client::resolve_daemon_local_ipc_endpoint` +
   `atm_http_runtime::preferred_local_client`).
2. **Lease refresh**: the existing `republish_if_missing` timer path in
   `crates/atm-graft/src/runtime.rs` additionally calls registration
   refresh on each tick — one timer, two maintenance actions during the
   dual-write period; refresh-only after AQ1.8.
3. **Daemon-unavailable resilience (lifecycle requirement)**: registration
   and refresh failures are logged, backed off, and retried on the next
   tick — they NEVER fail the bind, crash the receiver, or require any
   manual reset. A receiver that started while the daemon was down becomes
   registered automatically on the first successful tick after the daemon
   returns; a daemon that restarts finds the persisted lease already in
   SQLite and needs nothing from the receiver.
4. **Unregister on drop**: generation-checked `Drop` additionally sends
   unregister (best-effort, non-blocking — a missed unregister just leaves
   a lease that expires by window).
5. **Dual-write invariant**: the file record write remains byte-identical
   to today (including its known race — no interim behavior change hides
   inside this sprint); every consumer keeps reading the file until AQ1.7.

## Acceptance criteria

1. Bind with a reachable daemon registers exactly one lease matching the
   record file's endpoint/capability/generation (test uses a stub daemon
   endpoint or the AQ1.5 store behind a test router).
2. Bind with the daemon DOWN succeeds (receiver fully functional via file
   record), and the next refresh tick after the daemon returns registers
   the lease — no manual step (lifecycle test; deterministic clock per
   ADR-008).
3. Daemon restart with a live receiver: lease persists in the DB, refresh
   ticks keep `last_seen_at` advancing, delivery state never requires a
   receiver-side action (integration test over a reopened store).
4. Second same-host process attempting bind for the same (team, agent) is
   still stopped by the flock before any daemon call (existing
   `graft_receiver_ownership` tests pass unmodified).
5. Drop unregisters when generation matches; a stale lease left by a
   SIGKILLed receiver is replaced by the next bind (displacement rule from
   AQ1.5 exercised end-to-end).
6. `cargo test` workspace green on both CI lanes; file-record behavior
   byte-identical (existing graft tests pass unmodified).

## Non-closure / out of scope

- No consumer reads the daemon lease yet (AQ1.7).
- No file-record deletion or write-path change (AQ1.8).

## Dependencies

- must_follow: AQ1.5 (wire contract + store). Merge-forward trigger: AQ1.5
  dev push.
- parallel_safe: none claimed (same files as AQ1.7/AQ1.8 later touch).
