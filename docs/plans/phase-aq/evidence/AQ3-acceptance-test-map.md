# AQ3 acceptance test map

The AQ3 implementation tests are named at the behavior boundary:

| Criterion | Automated evidence |
| --- | --- |
| AC1 | `queue_drain::tests::idle_drain_delivers_oldest_then_next_transition_drains_next` |
| AC2 | `atm_storage_rusqlite::pending_nudge_store::tests::read_path_upsert_clears_the_pending_marker` |
| AC3 | `queue_drain::tests::recovery_sweep_replays_pending_after_restart` |
| AC4 | `queue_drain::tests::concurrent_transition_and_sweep_claim_once` |
| AC5 | `queue_drain::tests::shutdown_cancels_tracked_transition_and_releases_in_flight_claim`; `RecoverySweepHandle::shutdown` joins or aborts within the supplied deadline; no interval tick occurs before the maintenance cadence. |
| AC6 | `queue_drain::tests::shared_channel_precheck_skips_herdr_and_bare_cli_members`; `atm_storage::PendingNudgeStore::list_pending_members` is the sweep enumeration seam. |
| AC7 | `queue_drain_channel_allowed` is the single guard called by both transition drain and recovery sweep. Mechanical manifest enforcement (ATM-QA-103): `atm_architecture::member_state_transition_sink_boundary::{manifest_declares_the_expected_owner_and_at_least_one_forbidden_edge, manifest_forbidden_edges_are_absent_from_the_real_cargo_dependency_graph, only_the_manifest_declared_dependent_implements_the_sink_outside_its_owner_crate}`. |
| AC8 | `queue_drain::tests::recovery_sweep_isolates_one_member_failure_and_continues`; workspace `cargo test --workspace --all-targets`, targeted clippy, formatter, boundary, taxonomy, and function-length gates. Multi-OS CI citation (ATM-QA-102) recorded in the sprint doc's "Required validation" section. |

The tmux emitter is the live replacement Tokio/Axum path. The focused AQ3
drain tests use a recording selector to make the atomic claim, FIFO order, and
exact-marker clear deterministic without requiring a real tmux pane in CI.

## QA-1 remediation

The transition-drain task is retained in `TransitionDrainTracker`, cancelled
and joined during the replacement-daemon shutdown deadline. A
`ClaimedPendingNudge` drop guard releases an interrupted in-flight claim. The
recovery sweep records a member failure, keeps that member requeued, and
continues to the remaining candidates. All queue-marker handoff cleanup goes
through `atm_core::nudge_dispatch::clear_queue_marker_after_handoff`; the
architecture gate `queue_marker_handoff_clear_has_one_core_owner` enforces
that ownership mechanically.

## Live loopback status

The requested real daemon/tmux transcript remains pending on this workstation
(the host already has the shared daemon owner lock, and the documented `m5`
alias is not resolvable from here). ATM-QA-101 adds
`scripts/phase-aq/run_aq3_tmux_idle_drain_evidence.py`, a clean-runner
harness mirroring `run_aq25_queue_delivery_trigger_evidence.py`'s structure,
registered in `.github/workflows/phase-aq-evidence.yml`'s
`EVIDENCE_DIR_BY_SCRIPT`. It drives a real owned `atm-daemon` (launched with
`--peer-wire-security plaintext-test`), a real `atm` CLI, and a real scratch
tmux server (`tmux -L aq3-<rand>`, bridged to the daemon's unqualified `tmux`
invocations via `TMUX=<socket_path>,0,0`) through the actual idle-transition
drain path (`DrainOnTransitionSink`), asserting FIFO drain order via
`tmux capture-pane`, single-drain-per-transition via the
`queue_messages_drained_total` health counter, and immediate steer-kind
delivery. Its own unit tests live in
`scripts/phase-aq/test_run_aq3_tmux_idle_drain_evidence.py`, now discovered
by `.just/run_lint.py pytests`. No live result is represented as passing
evidence until the harness produces one on a clean runner or dedicated
account; this remains an open item until that dispatch completes.

### Clean-runner dispatch 2026-08-27: harness race, not an AQ3 bug

Run [`33125702152`](https://github.com/randlee/atm-core/actions/runs/33125702152)
(branch-ref dispatch, ubuntu-latest and macos-latest, identical result on
both; windows-latest correctly recorded `skipped_no_tmux`): daemon readiness,
tmux roster registration, steer-kind immediate delivery, both queue-kind
sends, and `fifo_order_confirmed=true` all passed; `single_drain_per_transition_confirmed=false`
because `idle_transition_drain_one`/`_two` read `queue_messages_drained_total`
as `0` right after `wait_for_pane` returned.

Root cause, confirmed from the downloaded JSON transcripts (`AQ3/tmux-idle-drain-clean-runner-{linux,macos}.json`):
this was a **harness read race, not an AQ3 drain defect**.
`TokioTmuxReceivedHook::emit_received_message` makes the rendered nudge text
visible in the pane on its *first* of three sequential `tmux send-keys`
calls; `queue_drain::drain_one` only clears the pending marker and calls
`RuntimeHealth::record_queue_message_drained()` *after* the second and third
`send-keys` calls, separated by the deliberate 275ms
`TMUX_DOUBLE_ENTER_DELAY` (`atm_core::boundary::TMUX_DOUBLE_ENTER_DELAY`).
The harness read the counter via a single immediate `atm doctor --json` call
right after `wait_for_pane` returned, which reliably beat that tail latency
on a fast CI runner. The macOS transcript proves this precisely: its two
reads returned `0` then `1` — drain one's own increment hadn't landed when
*its* read fired, but had landed by the time drain two's (earlier, separate)
read fired, exactly the staggered-read signature of a race, not a stuck
counter (which would have stayed `0` twice, or shown a nonzero
`queue_drain_failures_total` from a real failure — neither transcript did).
The write-time planner itself (`PreparedWrite::build_received_hook_dispatches`,
`crates/atm-core/src/write/pipeline.rs`) also confirms nothing else could
have delivered the pane content early: a `Deferred`/queue-kind write to a
`local_tmux_post_send` recipient returns an empty dispatch list at write
time (suppressed), so the pane content only ever reaches the tmux pane
through the drain claim this transcript was trying to observe.

Fix (harness-only, no AQ3 production change): `wait_for_drained_counter_at_least`
polls `queue_messages_drained_total` until it reaches the expected value or
`--timeout` elapses, instead of reading it once. Regression coverage:
`scripts/phase-aq/test_run_aq3_tmux_idle_drain_evidence.py::{test_wait_for_drained_counter_polls_past_the_tmux_double_enter_tail_latency,
test_wait_for_drained_counter_times_out_returning_the_last_observed_value}`.
