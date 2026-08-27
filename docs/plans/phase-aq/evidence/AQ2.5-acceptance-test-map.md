# AQ2.5 acceptance-test map

Each acceptance criterion has a named runnable test or validation command.
Updated for the QA-1 fix cycle (findings AQ25-CRIT-001, ATM-QA-001..009,
RBQA-F002, AQ25-CI-001, and the minors).

1. Heartbeat CLI surface and `RuntimeHealth` projection:
   `heartbeat_cli_parses_activity_team_and_actor_flags` and
   `heartbeat_cli_rejects_a_missing_activity` (real clap parsing, not enum
   arity) in `crates/atm/src/commands/internal_heartbeat.rs`, plus
   `heartbeat_route_drives_runtime_health_member_state_transitions_with_a_deterministic_clock`
   (a deterministic, non-wall-clock `observed_at` proving the existing
   Heartbeat route drives `RuntimeHealth`'s member-state projection end to
   end) and `authenticated_heartbeat_is_retained_without_affecting_runtime_readiness`
   (now also asserts `response.state`) in
   `crates/atm-http-runtime/src/storage_and_nudge_router.rs`. AQ3's own
   observation sink does not exist yet (AQ2.5 is upstream of AQ3 in the
   dependency chain); this AC is covered as far as it is actually
   implementable today, not against a sink that does not exist.
2. Hook debounce and lifecycle cancellation:
   `test_stop_pull_blocks_with_literal_json_and_schedules_idle`,
   `test_pre_tool_use_cancels_debounced_stop_timer`,
   `test_stop_debounce_expiry_sends_exactly_one_idle_heartbeat` (asserts the
   debounced expiry actually calls `_internal-heartbeat --activity idle`,
   not merely that the debounce window elapses), and
   `test_empty_stop_proceeds_without_output`, all in
   `scripts/hooks/test_queue_hooks.py::QueueHookTests`.
3. FIFO ordering, mixed-kind drain, and overflow:
   `queue_items_drain_oldest_one_at_a_time`,
   `steer_items_all_drain_with_one_queue_item`, and
   `overflow_drops_oldest_and_counts_the_drop` (now drains the full
   post-overflow window and asserts digit 0 specifically was evicted, not
   merely that some drop occurred) in
   `crates/atm-http-runtime/src/bare_cli_fifo.rs`.
4. Stop-pull loop termination and literal Claude block response:
   `test_literal_multi_stop_drain_sequence_terminates_on_empty` (a real,
   stateful multi-`Stop` sequence at the hook-script level: first Stop
   drains and blocks on the oldest message, the next Stop drains and blocks
   on the next, the Stop after that sees an empty FIFO and proceeds with no
   output) and `test_empty_stop_proceeds_without_output`, both in
   `scripts/hooks/test_queue_hooks.py::QueueHookTests`.
5. Caller identity surface and shared route codec:
   `queue_get_cli_has_no_target_member_argument`,
   `queue_get_next_route_round_trips_through_the_shared_codec` (wire codec),
   and `queue_get_next_router_rejects_a_caller_not_on_the_roster` (the real
   `queue_get_next` handler, not the codec alone, rejects a non-roster
   caller) in `crates/atm-http-runtime/src/storage_and_nudge_router.rs`.
6. Total classifier and selector gating, plus the migration case:
   `classify_delivery_channel_covers_all_four_rows`,
   `selector_routes_tmux_only_for_steer`,
   `selector_routes_both_herdr_kinds_through_the_injected_adapter`,
   `aq25_received_hook_manifest_matches_async_implementers`,
   `queue_get_next_router_drains_the_bare_cli_fifo_through_the_real_dispatch_path`
   (the real `StorageAndNudgeRouter::dispatch` path drains a pre-seeded
   FIFO entry, not just the FIFO helper in isolation), and
   `queue_get_next_router_drains_a_stale_fifo_entry_after_the_members_classification_changes`
   (the named migration-case test: pre-seed a FIFO entry, flip the
   member's roster input onto a tmux local backend, assert the real
   handler still drains the stale entry because `queue_get_next` never
   re-runs the classifier).
7. Bare-CLI marker handoff:
   `bare_cli_queue_pull_appends_and_clears_the_exact_pending_marker` and
   `aq25_crit_001_bare_cli_marker_clear_failure_does_not_fail_delivery`
   (regression test: a double-failing pending-marker store still yields
   `Success`, the FIFO append is observable, the marker-clear counter is
   incremented twice by the shared retry-once helper, and the durable
   marker is left set — disclosed as an orphaned-marker residual in the
   ADR-054 addendum, since bare-CLI members are never swept) in
   `crates/atm-daemon-bootstrap/src/received_hook_selector.rs`.
8. Bounded daemon-down/fail-open behavior for both CLI surfaces:
   `heartbeat_exits_ok_within_the_bounded_timeout_when_the_daemon_is_unavailable`
   (`crates/atm/src/commands/internal_heartbeat.rs`) and
   `queue_get_exits_ok_within_the_bounded_timeout_when_the_daemon_is_unavailable`
   (`crates/atm/src/commands/internal_queue_get.rs`). Both resolve a real,
   isolated endpoint path with nothing listening (a closed socket,
   simulating daemon unavailability) so the actual
   `preferred_local_client(...).execute(...)` connect-refusal path runs —
   not the earlier `resolve_daemon_local_ipc_endpoint()` short-circuit —
   and assert the command still exits `Ok(())` well inside
   `SAME_HOST_REQUEST_DEADLINE`.
9. Normative ADR-054 policy and its quality-mgr sign-off record:
   `aq25_adr_addendum_contains_normative_trigger_policy` and the new
   `aq25_adr_addendum_records_a_quality_mgr_sign_off_section` (asserts the
   `### AQ2.5 quality-mgr sign-off` heading and its table header row exist
   in the ADR — not just policy prose) in
   `crates/atm-architecture/tests/boundary_enforcement.rs`. The ADR carries
   a pending sign-off row for quality-mgr to fill on re-gate, mirroring
   AQ1 AC 1's ADR-054 gate.
10. Cross-platform hook lane and workspace validation:
    `just test-queue-hooks-python` (the harness-neutral `QueueHookTests`
    class; runs on all three CI matrix OSes, including Windows, since
    Claude Code runs on Windows), `just test-queue-hooks-python-codex`
    (the Codex-only `CodexQueueHookTests` class; the CI workflow invokes
    this recipe only on the ubuntu/macOS matrix legs, and the test class
    itself also self-skips on Windows via `unittest.skipIf` for direct/local
    runs), and `cargo test --workspace --all-targets`.
11. Boundary-manifest freshness:
    `aq25_received_hook_manifest_matches_async_implementers`.

## Other QA-1 fix-cycle changes

- **AQ25-CRIT-001**: `PullPendingReceivedHook::emit_received_message` now
  routes its post-append marker clear through the existing shared
  `clear_queue_marker_after_handoff` helper (introduced by AQ2, already on
  this branch) instead of a bare `?`. See test 7 above.
- **RBQA-F002**: `validate_heartbeat_member` is narrowed to
  `(&LocalServiceRuntime, &TeamName, &AgentName)`, mirroring the sibling
  `validate_graft_receiver_member`; `queue_get_next` no longer fabricates a
  throwaway `TeamMemberHeartbeatRequest` to satisfy the old signature.
- **RSH-002 / RSH-003**: documented as accepted tradeoffs in the ADR-054
  addendum's new "Accepted resilience tradeoffs" subsection (shared
  single-permit bridge latency risk under load; unbounded aggregate
  bare-CLI FIFO memory across members).
- **ATM-QA-001**: `docs/project-plan.md`'s Phase AQ status now carries an
  AQ2.5 branch/status entry alongside its sibling sprints.
- **ATM-QA-002**: `scripts/phase-aq/run_aq25_queue_delivery_trigger_evidence.py`
  is a real, runnable live-evidence runner (bare-CLI one-per-Stop drain +
  full steer drain against a real owned `atm-daemon` and the committed
  hook scripts) mirroring the accepted `run_hermes_atm_restart_matrix.py`
  (AQ1.9) pattern. It was executed for real in this environment; the
  committed transcript
  (`docs/plans/phase-aq/evidence/AQ2.5/queue-delivery-trigger-local.md`)
  records that this host has an ambient, already-running `atm-daemon`
  that legitimately owns the OS-account singleton runtime lock
  (`atm_core::home::current_host_runtime_scope` intentionally ignores
  `ATM_HOME`/`HOME`), so the runner safely refused to start a second
  daemon rather than risk that live session — exactly AQ1.9's own
  disclosed constraint. Positive-path evidence requires running the same
  script on a dedicated host/OS account with no ambient `atm-daemon` (for
  example, a CI job); this is disclosed as a residual, not fabricated.

The loopback Hermes evidence accepted for the AQ2 dependency is recorded in
`docs/plans/phase-aq/evidence/AQ2/hermes-atm-queue-steer-loopback.md`.
