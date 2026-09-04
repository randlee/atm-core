---
status: complete
branch: feature/aq-1-6-graft-receiver-registration-client
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/aq-1-6-graft-receiver-registration-client
---

# Sprint AQ1.6 — Graft Receiver Registration Client (Announce-at-Init)

Status: complete · Branch: `feature/aq-1-6-graft-receiver-registration-client` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Second graft connection-model sprint (see AQ1.5 for motivation and the
binding lifecycle requirement). The receiver announces its endpoint to the
daemon at bind time and keeps its lease fresh; the file record is still
written (dual-write) so nothing downstream changes until AQ1.7's cutover.

## Deliverables

1. **Announce at bind — client lives in `atm-graft`, never `atm-core`
   (closes critical-review B3)**: verified against the real tree —
   `crates/atm-core/Cargo.toml` depends on neither `atm-daemon-client` nor
   `atm-http-runtime`; `crates/atm-graft/Cargo.toml` already depends on both
   (and on `atm-core`). So `GraftReceiverListener::bind`
   (`crates/atm-core/src/graft.rs`) keeps binding `127.0.0.1:0`, acquiring
   the `ReceiverOwnershipGuard` flock, and generating the `LocalCapability`
   + `owner_generation` exactly as today — atm-core gains **no** new
   dependency and exposes only the lease *inputs*: two new accessors,
   `pub fn capability(&self) -> &LocalCapability` and
   `pub fn owner_generation(&self) -> &str`, alongside the existing
   `local_addr()`. The daemon registration call itself is new code in
   `crates/atm-graft/src/runtime.rs`'s `activate_graft_receiver`,
   immediately after a successful `GraftReceiverListener::bind`, using the
   client seam `GraftClient` already uses for send/read
   (`atm_daemon_client::resolve_daemon_local_ipc_endpoint` +
   `atm_http_runtime::preferred_local_client`, per
   `crates/atm-graft/src/lib.rs:195-203`).
2. **Lease refresh — starvation-proof and off today's crash-prone idle-only
   path (closes M7)**: today `listen_for_graft_nudges`'s
   `Ok(None) => handle_idle_graft_receiver(ctx, &listener, &mut
   last_record_recheck)?` (`crates/atm-graft/src/runtime.rs`) uses `?`, so a
   `republish_if_missing` failure propagates straight out of the accept
   loop and ends the receiver — the opposite of deliverable 3's "never
   crash" requirement. This sprint moves the elapsed check (now against
   AQ1.5's `GRAFT_LEASE_REFRESH_INTERVAL`) out of the idle-only branch to
   the top of the loop body in `listen_for_graft_nudges`, so it runs on
   **every** iteration regardless of the `Ok(Some)` / `Ok(None)` / `Err`
   arm — a receiver busy draining back-to-back nudges (which never reaches
   the idle branch today) now still refreshes on cadence. The
   refresh/republish call is handled the same non-propagating way
   `handle_graft_receiver_connection` errors already are
   (`if let Err(error) = ... { warn_runtime_error(...); }`, logged and the
   loop continues) — never `?`. During the dual-write period the same
   per-iteration check also drives the file republish (unchanged in
   effect); refresh-only after AQ1.8.
3. **Daemon-unavailable resilience (lifecycle requirement)**: registration
   and refresh failures — `GraftEndpointStoreError::Storage` and
   network/daemon-unavailable errors from the client call in
   `crates/atm-graft/src/runtime.rs` (deliverable 1) — are logged, backed
   off, and retried on the next tick (deliverable 2's per-iteration
   cadence) — they NEVER fail the bind, crash the receiver, or require any
   manual reset. A receiver that started while the daemon was down becomes
   registered automatically on the first successful tick after the daemon
   returns; a daemon that restarts finds the persisted lease already in
   SQLite and needs nothing from the receiver. (`AlreadyActive` is not
   expected on this path once the AQ1.5 amendment recorded in this pass's
   plan-finalization report lands — see deliverable 4 and Acceptance
   criterion 5.)
4. **Unregister on drop — `atm-graft`-owned wrapper, not `atm-core`'s
   `Drop` (closes B3 for the drop path too)**: `atm-core`'s
   `impl Drop for GraftReceiverListener` is unchanged — it still only
   removes the JSON record during the dual-write period (deliverable 6)
   and cannot call the daemon client. `crates/atm-graft/src/runtime.rs`
   introduces `struct RegisteredGraftReceiver { listener:
   GraftReceiverListener, owner_generation: String }`, constructed
   immediately after a successful bind + register; its `Drop` sends
   `GraftReceiverUnregister { team, agent, owner_generation }`
   best-effort/non-blocking (a missed unregister just leaves a lease that
   expires by window) before the inner `GraftReceiverListener` drops
   normally. Every runtime.rs call site currently holding a bare
   `GraftReceiverListener` (`activate_graft_receiver`,
   `rebind_graft_receiver`, and their test doubles) is updated to hold
   `RegisteredGraftReceiver` instead.
5. **Lock-path decoupling and `bind` signature change (required before
   AQ1.8 can delete the record path builders; answers critical-review B4's
   open question)**: introduce a dedicated
   `graft_receiver_lock_path_from_root(root, team, agent)` in
   `atm-core::graft` that derives the flock path independently of the
   JSON record path (same on-disk `.lock` location as today's
   `receiver_ownership_lock_path(record_path)`, so existing locks stay
   valid). `GraftReceiverListener::bind`'s signature changes from
   `bind(record_path: &Path, owner_chat_id: Option<ChatId>)` to
   `bind(graft_root: &Path, team: &TeamName, agent: &AgentName,
   owner_chat_id: Option<ChatId>)`; internally it derives the lock path via
   the new function and (dual-write period only) the record path via the
   existing `graft_receiver_record_path_from_root` — callers no longer
   construct either path themselves. `crates/atm-graft/src/runtime.rs`'s
   `GraftReceiverLoopContext.endpoint_path: PathBuf` is renamed to
   `graft_root: PathBuf` and **changes meaning** from "the record file
   path" to "the root directory `bind` derives both paths from"; the
   struct also gains `team: TeamName, agent: AgentName` fields (needed
   anyway by deliverable 1's registration call and deliverable 4's
   unregister). Every real call site migrates: `crates/atm-graft/src/lib.rs`'s
   `GraftSession::activate_with_observability` (~:390, production) and the
   bare-workspace activation test (~:784) stop calling
   `graft_receiver_record_path_from_root` and instead pass
   `(workspace_root, team, agent)` straight into the
   `GraftReceiverLoopContext`; `crates/atm-graft/src/runtime.rs`'s own
   `#[cfg(test)]` module (the `receiver_endpoint_path` helper and every
   `GraftReceiverListener::bind(&endpoint_path, ...)` call, ~:891 onward)
   and `crates/atm-core/src/graft.rs`'s own `#[cfg(test)]` module (every
   `GraftReceiverListener::bind(&record_path, ...)` call built from
   `graft_receiver_record_path_from_home`) migrate the same way. After this
   sprint, no code outside `atm-core/src/graft.rs` references
   `graft_receiver_record_path_*` — verified against the real tree: today
   those symbols are used at `crates/atm-core/tests/graft_receiver_ownership.rs:1,12`,
   `crates/atm-graft/src/runtime.rs:892`, and
   `crates/atm/src/commands/internal_nudge.rs:13,170` (critical-review B4);
   the first two migrate here to the new `bind` signature, the third is
   AQ1.7 deliverable 2's job (it doesn't bind a listener, it queries the
   daemon).
6. **Dual-write invariant**: the file record write remains byte-identical
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
5. **Displacement is immediate, not window-gated (closes I10)**: drop
   unregisters when generation matches (unchanged); a stale lease left by
   a SIGKILLed receiver is replaced by the very next bind's registration —
   because that bind's flock acquisition already OS-proves the prior owner
   is dead (deliverable 1's ordering: flock before register), the
   replacement must succeed on the first registration tick after the new
   process starts, not after `ACTIVE_LEASE_WINDOW` (15s) elapses (test with
   a deterministic clock per ADR-008: SIGKILL a receiver mid-window, bind
   a successor immediately, assert the new lease is live within one
   `GRAFT_LEASE_REFRESH_INTERVAL` tick). This acceptance criterion depends
   on the AQ1.5 amendment recorded in this plan-finalization pass's report
   (removing the window-gated `AlreadyActive` rejection from `register`
   for same-host, flock-proven callers).
6. Sustained-load refresh: a receiver kept continuously busy (back-to-back
   accepted connections, no idle iterations) for longer than
   `ACTIVE_LEASE_WINDOW` still refreshes on cadence and its lease never
   becomes displaceable (deterministic clock per ADR-008).
7. **No-crash-on-refresh-failure (closes M7)**: a refresh/republish
   failure injected mid-loop is logged and does not terminate
   `listen_for_graft_nudges` — the receiver keeps accepting connections on
   subsequent iterations after the injected failure (deterministic test
   double for the store/file call).
8. `RegisteredGraftReceiver::drop` sends unregister for its own
   `owner_generation` only; dropping an already-superseded wrapper (an old
   generation after a newer bind has taken over) is a no-op against the
   newer lease — the store's `NotOwner` rejection is swallowed the same
   best-effort way as any other unregister failure (mirrors today's
   generation-checked Drop test / `GraftReceiverListener`
   successor-replacement test).

## Required validation

- `cargo test` workspace green on both CI lanes; file-record behavior
  byte-identical (existing graft tests pass unmodified).

## Non-closure / out of scope

- No consumer reads the daemon lease yet (AQ1.7).
- No file-record deletion or write-path change (AQ1.8).
- **Production composition-root wiring is AQ1.7's job (critical-review
  I8)**: this sprint's registration/refresh/unregister calls succeed
  against any router that has the AQ1.5 store attached (test routers, and
  — once the AQ1.5 amendment below lands — AQ1.5's own handler tests);
  until AQ1.7 threads the real rusqlite store into the actually-running
  daemon's `LocalServiceRuntime` (bootstrap composition root), a live
  production daemon's register calls fail with a storage-unavailable-style
  error, which deliverable 3 already treats as harmless (logged, backed
  off, retried) — nothing downstream depends on registration succeeding
  before AQ1.7's cutover (dual-write, deliverable 6).

## Dependencies

- must_follow: AQ1.5 (wire contract + store), **as amended by this
  plan-finalization pass** — see this pass's report for the exact AQ1.5
  deliverable-4 / Contract / read-route / encoding changes this sprint and
  AQ1.7 depend on (critical-review I9, I10, I12). Merge-forward trigger:
  AQ1.5 dev push.
- parallel_safe: AQ2.6, AQ2.7 (Herdr — disjoint files; 2026-08-26 reorder).
  None claimed within the graft chain (same files as AQ1.7/AQ1.8 later touch).
