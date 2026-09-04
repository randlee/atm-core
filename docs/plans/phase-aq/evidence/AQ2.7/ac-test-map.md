# AQ2.7 acceptance-test map

The AC tests below run through the production `HerdrQueueWakePump` path,
SQLite-backed pending-nudge storage, and the injected
`atm_herdr::testing::FakeHerdrProcessAdapter`. They do not assert constants as
their sole behavior.

| AC | Test | Observable assertion |
|---|---|---|
| 1 | `herdr_queue_wake::tests::ac01_fifo_per_member_via_claim` | Three durable markers for one member result in one oldest-first prompt and a remaining pending marker. |
| 2 | `herdr_queue_wake::tests::ac02_burst_cap_is_sixteen_successful_prompts` | Seventeen idle members yield exactly 16 prompts; the seventeenth marker is still at attempt 0. |
| 3 | `herdr_queue_wake::tests::ac03_session_grouping_is_part_of_the_poll_contract` | Two session buckets produce exactly two recorded `list` calls with distinct sessions. |
| 4 | `herdr_queue_wake::tests::ac04_shutdown_send_stops_pump_before_drain_completes` | A live shutdown sender interrupts a gated prompt, the task joins within one second, and no second prompt occurs. |
| 5 | `herdr_queue_wake::tests::ac05_fake_adapter_breaker_error_does_not_prompt` | A real `list` infrastructure error produces no prompt and records an open-breaker outcome. |
| 6 | `herdr_queue_wake::tests::ac06_blocked_race_releases_pending_with_zero_injected_bytes`; `herdr_queue_wake::tests::ac06_not_found_family_releases_without_input`; `herdr_queue_wake::tests::ac06_consecutive_release_bound_requeues_after_ten` | Blocked and not-present-family post-claim errors release the exact claim with zero retry debt; outcomes 1–10 retain the marker and the 11th requeues once, consumes one attempt, and resets the release counter. |
| 7 | `herdr_queue_wake::tests::ac07_absent_members_are_not_presented_as_idle` | An absent listed target is not prompted and its durable marker remains pending. |
| 8 | `herdr_queue_wake::tests::ac08_dispatch_selector_is_used_by_tick_once` | A successful tick reaches the fake emitter through the selector and clears the pending set. |
| 9 | `herdr_queue_wake::tests::ac09_fake_adapter_never_needs_wait_for_queue_wake` | The real tick records a `list`/`prompt` path and zero `wait` calls. |
| 10 | `herdr_queue_wake::tests::ac10_herdr_statuses_update_runtime_health_states` | A polled working agent is observable as `Active` with `HerdrPoll` provenance. |
| 11 | `herdr_queue_wake::tests::ac11_claim_drop_guard_releases_marker_on_cancellation` | Cancellation of a gated prompt restores the marker and preserves attempt 0. |
| 12 | `herdr_queue_wake::tests::ac12_cursor_contract_is_rotation_not_reordering` | Twenty members prompt 16 on tick one, then the remaining four on tick two after cursor rotation. |

Focused command:

```text
cargo test -p atm-http-runtime herdr_queue_wake::tests -- --nocapture
```

QA-4 focused command:

```text
cargo test -p atm-http-runtime herdr_queue_wake::tests::ac06 -- --nocapture
```
