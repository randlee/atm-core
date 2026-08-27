# AQ2 acceptance-test map

Each AQ2 acceptance criterion has a named runnable regression or validation
command. QA-2 additions are included in the final two rows.

1. Queue graft handoff clears the exact marker and preserves steer:
   `queue_graft_handoff_clears_only_the_handed_message_marker` and
   `selector_routes_tmux_only_for_steer` in
   `crates/atm-daemon-bootstrap/src/received_hook_selector.rs`.
2. Write-time and sweep-dispatched failures retain/requeue correctly:
   `failed_queue_graft_handoff_retains_marker_at_attempt_zero` and
   `sweep_dispatched_queue_graft_failure_reports_for_caller_requeue`.
3. Hermes additive kind routes to the right channel:
   `test_callback_routes_steer_kind_to_hermes_steer_mode` and
   `test_callback_enqueues_notice_then_internal_telegram_event` in the
   Hermes bridge test suite.
4. The checked-in loopback harness passes both channel assertions:
   `python3 .just/run_hermes_graft_bridge_tests.py` (18 tests).
5. Successful graft delivery with a failing marker clear remains successful,
   retries exactly once, leaves the marker set, and separates counters:
   `aq2_crit_001_successful_handoff_retries_marker_clear_failure_without_failing_delivery`.
6. A deferred marker failure retries once and records the failure without
   replacing the admitted write result:
   `aq2_crit_002_marker_failure_retries_once_and_preserves_write_success`.

The complete Rust validation commands are recorded in the PR body and include
the core, HTTP-runtime, daemon-bootstrap, boundary, formatting, clippy, and
Python bridge lanes.
