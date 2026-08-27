# AQ3 acceptance test map

The AQ3 implementation tests are named at the behavior boundary:

| Criterion | Automated evidence |
| --- | --- |
| AC1 | `queue_drain::tests::idle_drain_delivers_oldest_then_next_transition_drains_next` |
| AC2 | `atm_storage_rusqlite::pending_nudge_store::tests::read_path_upsert_clears_the_pending_marker` |
| AC3 | `queue_drain::tests::recovery_sweep_replays_pending_after_restart` |
| AC4 | `queue_drain::tests::concurrent_transition_and_sweep_claim_once` |
| AC5 | `RecoverySweepHandle::shutdown` joins or aborts within the supplied deadline; no interval tick occurs before the maintenance cadence. |
| AC6 | `queue_drain::tests::shared_channel_precheck_skips_herdr_and_bare_cli_members`; `atm_storage::PendingNudgeStore::list_pending_members` is the sweep enumeration seam. |
| AC7 | `queue_drain_channel_allowed` is the single guard called by both transition drain and recovery sweep. |
| AC8 | Workspace `cargo test --workspace --all-targets`, targeted clippy, formatter, boundary, taxonomy, and function-length gates. |

The tmux emitter is the live replacement Tokio/Axum path. The focused AQ3
drain tests use a recording selector to make the atomic claim, FIFO order, and
exact-marker clear deterministic without requiring a real tmux pane in CI.
