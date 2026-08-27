# AQ2.5 acceptance-test map

Each acceptance criterion has a named runnable test or validation command.

1. Heartbeat CLI and runtime projection:
   `heartbeat_cli_accepts_all_three_activity_values` and
   `authenticated_heartbeat_is_retained_without_affecting_runtime_readiness`.
2. Hook debounce and lifecycle cancellation:
   `test_stop_pull_blocks_with_literal_json_and_schedules_idle`,
   `test_pre_tool_use_cancels_debounced_stop_timer`, and
   `test_empty_stop_proceeds_without_output` in
   `scripts/hooks/test_queue_hooks.py`.
3. FIFO ordering, mixed-kind drain, and overflow:
   `queue_items_drain_oldest_one_at_a_time`,
   `steer_items_all_drain_with_one_queue_item`, and
   `overflow_drops_oldest_and_counts_the_drop`.
4. Stop-pull loop termination and literal Claude block response:
   `test_stop_pull_blocks_with_literal_json_and_schedules_idle` and
   `test_empty_stop_proceeds_without_output`.
5. Caller identity surface and shared route codec:
   `queue_get_cli_has_no_target_member_argument` and
   `queue_get_next_route_round_trips_through_the_shared_codec`.
6. Total classifier and selector gating:
   `classify_delivery_channel_covers_all_four_rows`,
   `selector_routes_tmux_only_for_steer`,
   `selector_routes_both_herdr_kinds_through_the_injected_adapter`, and
   `aq25_received_hook_manifest_matches_async_implementers`.
7. Bare-CLI marker handoff:
   `bare_cli_queue_pull_appends_and_clears_the_exact_pending_marker`.
8. Bounded daemon-down/fail-open hook behavior:
   `test_empty_stop_proceeds_without_output` and
   `test_codex_stop_consumes_queue_without_claude_block_json`.
9. Normative ADR-054 policy:
   `aq25_adr_addendum_contains_normative_trigger_policy`.
10. Cross-platform hook lane and workspace validation:
    `python3 scripts/hooks/test_queue_hooks.py -v`,
    `just test-queue-hooks-python` where `.bootstrap-venv` is provisioned,
    and `cargo test --workspace --all-targets`.
11. Boundary-manifest freshness:
    `aq25_received_hook_manifest_matches_async_implementers`.

The loopback Hermes evidence accepted for the AQ2 dependency is recorded in
`docs/plans/phase-aq/evidence/AQ2/hermes-atm-queue-steer-loopback.md`.
