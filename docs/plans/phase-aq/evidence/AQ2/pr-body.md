## Summary

- Add additive steer/queue kind to graft wire requests, HostNudge, and PyNudge.
- Route graft queue dispatches through the published receiver and clear only the handed pending marker on successful handoff.
- Retain markers and attempts on write-time failure; record structured failure health observations.
- Route Hermes callbacks to the corresponding steer or queue mode.
- Fix QA-1 marker-clear result conflation and retry deferred marker writes once with a doctor-visible counter.
- Split graft delivery failures from post-handoff marker-clear failures in doctor/status.
- Add required AQ2 Hermes loopback evidence: `docs/plans/phase-aq/evidence/AQ2/hermes-atm-queue-steer-loopback.md`

## Acceptance criteria → named runnable test

1. Queue graft handoff clears the exact marker and preserves steer: `queue_graft_handoff_clears_only_the_handed_message_marker`; `selector_routes_tmux_only_for_steer`.
2. Write-time and sweep-dispatched failures retain/requeue correctly: `failed_queue_graft_handoff_retains_marker_at_attempt_zero`; `sweep_dispatched_queue_graft_failure_reports_for_caller_requeue`.
3. Hermes additive kind routes to the right channel: `test_callback_routes_steer_kind_to_hermes_steer_mode`; `test_callback_enqueues_notice_then_internal_telegram_event`.
4. Required loopback validation: `python3 .just/run_hermes_graft_bridge_tests.py` (18 tests, including both channel assertions).
5. Successful graft delivery plus failing marker clear remains successful, retries exactly once, leaves the marker set, and splits counters: `aq2_crit_001_successful_handoff_retries_marker_clear_failure_without_failing_delivery`.
6. A deferred marker failure retries once, increments `queue_marker_set_failures_total`, and preserves the admitted write result: `aq2_crit_002_marker_failure_retries_once_and_preserves_write_success`.

Full named map: `docs/plans/phase-aq/evidence/AQ2/acceptance-test-map.md`.

## Validation

- `cargo test -p agent-team-mail-core -p atm-http-runtime -p atm-daemon-bootstrap`
- `cargo clippy -p agent-team-mail-core -p atm-http-runtime -p atm-daemon-bootstrap --all-targets -- -D warnings`
- `cargo fmt --all -- --check`
- `python3 .just/run_hermes_graft_bridge_tests.py` (PASS; 18 tests)
- `python3 scripts/test_atm_graft_python.py`
- `python3 .just/lint_boundaries.py`
- nudge taxonomy, manifests/version sync, and function-length checks

The aggregate lint wrapper has local baseline failures for the absent `.bootstrap-venv` interpreter and pre-existing SC-boundary cycle findings; its Python test lane passed.
