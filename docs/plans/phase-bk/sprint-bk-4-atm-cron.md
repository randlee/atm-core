# Sprint bk-4 — atm-cron crate

closure_type `boundary`; target_boundary `BOUNDARY-CronScheduler` (the
`atm-cron` crate behind bk-2's sealed `atm_core::boundary::CronScheduler`
trait); branch `sprint/bk-4-atm-cron`, PR → the stack top after bk-3 is
rebased (`sprint/bk-3-hook-runner`); difficulty normal; recommended_agent
`arch-ctm`, recommended_model `gpt-5-codex`; parallel_safe with bk-3
(disjoint owned paths). Consumes bk-1 (the store, row types,
`StopCondition`, `record_run -> Result<bool>`, `CronExprInvalid`) and bk-2
(the hook types, the sealed `HookRunner`, `CronScheduler`,
`HookActionDispatcher` traits and their doubles), all through
`atm_core::boundary`. Governing text: `design.md` §5 (D-4, D-g, D-9),
§9.7; `REQ-P-CRON-001` §24.1, `REQ-P-CRON-002` §24.2; ADR-069 D1, D2, D3,
D4, ADR-068 D2, ADR-065, ADR-067.

## Scope boundary

This sprint owns one new workspace crate, `crates/atm-cron`: the only
implementation of `CronScheduler` and the worker that turns active
`cron_schedule` rows into hook runs. It composes nothing: the bootstrap
(bk-5) builds `AtmCron` and installs it in the router. It adds no
storage, protocol, CLI, router or doctor code.

Owned paths (new unless marked edited):

- `crates/atm-cron/Cargo.toml`, `crates/atm-cron/src/lib.rs`,
  `src/scheduler.rs` (`AtmCron`, the trait impl, `store_call`),
  `src/worker.rs` (the loop, `Slot`), `src/clock.rs` (`Clock`,
  `SystemClock`, and `TokioClock` behind `test-utils`), `src/jitter.rs`
  (`JitterSource`, `SystemJitter`, `SeqJitter` behind `test-utils`,
  `assign`), `src/tick.rs`
  (one run, `CronRunOutcome`), `src/expr.rs` (the grammar), under
  `crates/atm-cron/`; `crates/atm-cron/tests/worker.rs`,
  `crates/atm-cron/tests/expr.rs`
- `Cargo.toml` (edited: `"crates/atm-cron"` in `[workspace] members`
  after `"crates/atm-storage-rusqlite"`; `atm-cron = { path =
  "crates/atm-cron", version = ... }` like its neighbours and `cron =
  "0.17"` in `[workspace.dependencies]`; `getrandom = { workspace = true }`
  in the crate manifest only)
- `boundaries/atm-cron/cron-scheduler.toml` (new, text below),
  `.just/lint-config.toml` (edited: one
  `[[boundaries.manifest_dependency_allowlists]]` entry for
  `crates/atm-cron/Cargo.toml` with `boundary_record_path =
  "boundaries/atm-cron/cron-scheduler.toml"` and `allowed_dependencies =
  ["async-trait", "atm-core", "atm-runtime-test-support", "chrono", "cron",
  "getrandom", "serde_json", "tokio", "tracing"]`, and one
  `[[boundaries.global_dependency_ownership]]` entry with `dependency =
  "cron"`, `allowed_manifest_paths = ["crates/atm-cron/Cargo.toml"]`,
  `allowed_source_roots = ["crates/atm-cron/src"]`, in the format of the
  `rusqlite` entry)
- `boundaries/atm-core/hook-runner.toml`, `boundaries/atm-core/cron-scheduler.toml`,
  `boundaries/atm-core/hook-action-dispatcher.toml`,
  `boundaries/atm-storage/cron-schedule-store.toml` (edited: `atm-cron`
  added to `allowed_dependents`)
- `crates/atm-architecture/tests/boundary_enforcement.rs` (edited: the
  eight forbidden edges below in `EXPECTED_FORBIDDEN_EDGES`)
- `release/sc-publish-consumer-input.json` (edited: an `atm-cron` crate
  entry with `publish_order` 12, `required` true, `publish` true;
  `atm-daemon-bootstrap`, `atm-daemon`, `atm-graft`, `agent-team-mail`
  renumbered 13..16) and `release/publish-artifacts.toml` (re-rendered
  from `release/publish-artifacts.toml.j2`, the same edit)

## Public contract

```rust
// crates/atm-cron/src/lib.rs
#![forbid(unsafe_code)]
mod clock; mod expr; mod jitter; mod scheduler; mod tick; mod worker;
pub use clock::{Clock, SystemClock}; pub use jitter::{JitterSource, SystemJitter};
#[cfg(feature = "test-utils")] pub use {clock::TokioClock, jitter::SeqJitter};
pub use scheduler::{AtmCron, AtmCronDeps, CronWorkerHandle};

// crates/atm-cron/src/clock.rs
pub trait Clock: Send + Sync { fn now(&self) -> DateTime<Utc>; }
pub struct SystemClock;                                     // Utc::now()
#[cfg(feature = "test-utils")] pub struct TokioClock { epoch: DateTime<Utc>, start: tokio::time::Instant }   // epoch + (tokio::time::Instant::now() - start): moves with a paused tokio clock

// crates/atm-cron/src/jitter.rs  (design.md §5 D-r, Rand 2026-10-18: "random over remaining slots")
pub trait JitterSource: Send + Sync { fn below(&self, n: u32) -> u32; }   // uniform in [0, n), n >= 1
pub struct SystemJitter;   // getrandom::fill of a u32, rejection sampling (no modulo bias); getrandom is the workspace's existing 0.3 entry
#[cfg(feature = "test-utils")] pub struct SeqJitter { pub values: Mutex<VecDeque<u32>>, pub calls: Mutex<Vec<u32>> }   // records n, returns the next value (0 when empty)
/// J = clamp(period_s / 2, 1, 1800), period_s = seconds between the first two occurrences after now (J = 1 with one occurrence);
/// free = [0, J) minus the jitter_s of `held`; pick free[src.below(free.len())], or src.below(J) when free is empty.
pub(crate) fn assign(schedule: &cron::Schedule, now: DateTime<Utc>, held: &[u32], src: &dyn JitterSource) -> u32;

// crates/atm-cron/src/expr.rs  (design.md D-g)
pub(crate) fn parse(cron_expr: &str, now: DateTime<Utc>) -> Result<cron::Schedule, AtmError>;
// field count after trim (macros exempt) must be 6 or 7: five gives cron_expr_invalid("five-field expressions are not accepted; use 6 or 7 fields, seconds first, or @hourly..@yearly");
// cron::Schedule::from_str errors carry the parser's text; schedule.after(&now).next() None gives cron_expr_invalid("expression has no future occurrence").

// crates/atm-cron/src/scheduler.rs
pub struct AtmCronDeps {
    pub store: Arc<dyn CronScheduleStore>, pub runner: Arc<dyn HookRunner>,   // the router's one runner (bk-5); this crate never builds one (REQ-BK-D3)
    pub dispatcher: Arc<dyn HookActionDispatcher>,                            // the base router, below the filter (design.md §9.9)
    pub clock: Arc<dyn Clock>, pub host: Option<HostName>,                    // host = AtmConfig.local_host for HookCaller.host
    pub jitter: Arc<dyn JitterSource>,                                        // SystemJitter in bk-5; SeqJitter in tests
}
pub struct AtmCron { deps: AtmCronDeps, reload: tokio::sync::watch::Sender<u64> }
impl AtmCron {
    pub fn new(deps: AtmCronDeps) -> Self;
    pub fn validate_expression(&self, cron_expr: &str) -> Result<(), AtmError>;       // expr::parse at clock.now()
    /// unknown id: Err(validation "no such schedule"); ended: Err(validation "schedule ended");
    /// in flight in the worker: Ok("skipped_in_flight"); else tick::run_one and Ok(the recorded outcome string).
    pub async fn run_now(&self, id: CronScheduleId) -> Result<String, AtmError>;
    /// Loads active_all() once (Err aborts startup in bk-5), spawns the loop, returns the handle.
    pub async fn start_worker(self: &Arc<Self>) -> Result<CronWorkerHandle, AtmError>;
}
async fn store_call<T: Send + 'static>(store: Arc<dyn CronScheduleStore>, f: impl FnOnce(&dyn CronScheduleStore) -> Result<T, AtmError> + Send + 'static) -> Result<T, AtmError>;   // tokio::task::spawn_blocking
impl atm_core::boundary::sealed::Sealed for AtmCron {}
#[async_trait] impl CronScheduler for AtmCron {
    /// expr::parse; StopCondition::Until { at } with at <= now: cron_expr_invalid("until is not in the future") (the timeout cap is the route's, bk-3);
    /// jitter_s = jitter::assign(.., held = jitter_s of active_all() rows with the same cron_expr, other than this (team, phase, name)); then store.start (an ended name is updated in place keeping its id, design D-i; an active duplicate is refused by the store); then reload bump.
    async fn start(&self, schedule: NewCronSchedule) -> Result<CronSchedule, AtmError>;
    async fn end(&self, team: &TeamName, phase: &PhaseId, name: Option<&str>) -> Result<Vec<CronSchedule>, AtmError>;   // store.end then reload bump
    async fn list(&self, team: Option<&TeamName>, phase: Option<&PhaseId>) -> Result<Vec<CronSchedule>, AtmError>;
}
pub struct CronWorkerHandle { shutdown: Mutex<Option<oneshot::Sender<()>>>, worker: Mutex<Option<JoinHandle<()>>> }
impl CronWorkerHandle { pub async fn shutdown(&self, deadline: tokio::time::Instant); }   // stop scheduling, await in-flight runs until deadline, abort after

// crates/atm-cron/src/worker.rs  (pub(crate))
pub(crate) const MAX_SLEEP_SLICE: Duration = Duration::from_secs(30);
struct Slot { schedule: CronSchedule, next: Option<DateTime<Utc>>, in_flight: Option<JoinHandle<CronRunOutcome>> }   // next = occurrence + jitter_s, the first such time after now
// loop: select! { sleep(min(MAX_SLEEP_SLICE, earliest wake - now)) => run every due slot, reload.changed() => merge active_all() by id
// (keep in_flight and next of a still-active slot), finished in_flight => Stopped drops the slot, any other outcome recomputes next, shutdown => break }.
// wake = min(next, until_at) for StopCondition::Until; `now` in run_one is the jittered fire time. A loaded row with no future occurrence: store.record_stop(id, now, "no future occurrence").
// A JoinError from a panicking run: outcome Exit with "hook runner panicked: <payload>", the worker keeps running (Rand panic ruling).

// crates/atm-cron/src/tick.rs  (pub(crate))
enum CronRunOutcome { Ok, Refused(String), Actions(usize), SkippedInFlight, Exit(String), Timeout, Malformed, ActionErr(AtmErrorCode), Stopped(String), StopCheckFailed(AtmErrorCode) }
// as_str: "ok" | "refused: <msg>" | "actions: <n>" | "skipped_in_flight" | "exit" | "timeout" | "malformed" | "action_err: <code>" | "stopped: <reason>" | "stop_check_failed: <code>"
pub(crate) async fn run_one(cron: &AtmCron, schedule: &CronSchedule) -> CronRunOutcome;
```

`run_one` runs these steps in order.

1. **Stop check.**
   - `Until { at }` with `now >= at` calls `record_stop(id, now, "until
     reached")` and returns `Stopped`.
   - `Hook` calls `runner.stop` with `HookRequest::for_cron(reg, caller,
     cron.stop, &args)` under the deadline `now + timeout_ms +
     HOOK_ACTION_RESERVE_MS`, so the runner's §9.7 `min` gives exactly the
     row's `timeout_ms` (up to `CRON_TIMEOUT_MAX_MS`).
     `{"stop": true, "reason": r}` gives `record_stop` and `Stopped(r)`.
     `{"stop": false}` continues to step 2. An `Err` gives
     `StopCheckFailed(code)`, and the row stays active.
2. **Run.** Call `runner.run` with `HookRequest::for_cron(reg, caller,
   cron.tick, &args)`.
   - The caller is `HookCaller { identity: created_by, team, host }`.
   - The registration passed to the runner is built from the row.
   - Its deadline is the same as the stop check's: the row's `timeout_ms`,
     never the module's task-hook cap.
   - The request's `custom` is the row's `custom` and `request` is
     `{"args": [...], "created_by": <created_by>}` (bk-2 `for_cron`).
3. **Reply.**
   - `Refused { message }` gives `Refused`, and nothing is dispatched.
   - `PassThrough` and `NotApplicable` give `Ok` (nothing to do).
   - `Handled(list)` calls `dispatcher.dispatch_actions(cron.tick, team,
     phase, created_by, list, None)`. The `None` deadline gives each action
     `now + SERVER_REQUEST_BUDGET`. The result is `Actions(n)` or
     `ActionErr(code)`.
   - A runner `Err` maps by its code to `Exit`, `Timeout` or `Malformed`,
     and nothing is dispatched.
4. **Record.** Call `record_run(id, now, outcome.as_str())`. When it
   returns `false`, the row was ended mid-run: the outcome is logged and
   nothing is written.

Log entries (ADR-067, `tracing::info!`): `subsystem = "atm_cron"`, `action
= "stop_check" | "tick"`, `outcome` as `as_str` without its suffix, plus
`team`, `phase`, `hook`, `schedule`, `elapsed_ms`, and `reason` on
`stopped`. Per-action entries are the dispatcher's (bk-3).

`boundaries/atm-cron/cron-scheduler.toml` (format of
`boundaries/atm-herdr/herdr-process-adapter.toml`):

```toml
boundary_id = "BOUNDARY-CronScheduler"
owner_package = "atm-cron"
owner_crate_path = "atm_cron"
name = "CronScheduler"

[public]
trait = "CronScheduler"
facade = "AtmCron"
notes = "The only CronScheduler implementation (Rand 2026-10-18, PR #1877: own crate, simple trait). The trait is sealed in atm_core::boundary (bk-2)."

[implementation]
type = "AtmCron"
module = "atm_cron::scheduler"
visibility = "public"
constructor = "public"

[composition]
roots = ["atm_daemon_bootstrap"]

[ownership]
io_owns = ["cron_expr_parse", "schedule_next_computation", "stop_condition_check", "cron_tick", "reload_signal", "cron_run_record", "jitter_assignment"]
io_forbidden = ["sqlite", "process_spawn", "http_server"]

[dependencies]
allowed_dependents = ["atm-daemon-bootstrap"]
allowed_dependencies = ["async-trait", "atm-core", "chrono", "cron", "getrandom", "serde_json", "tokio", "tracing"]
allowed_dev_dependencies = ["atm-core", "atm-runtime-test-support", "tokio"]
forbidden_edges = ["atm-cron -> atm-storage", "atm-cron -> atm-http-runtime", "atm-cron -> atm-daemon-bootstrap", "atm-cron -> atm-storage-rusqlite", "atm-cron -> atm-herdr", "atm-cron -> atm-graft", "atm-core -> atm-cron", "atm-http-runtime -> atm-cron"]

[references]
scope = "outside_owner_crate"
forbidden = []

[contracts]
request_types = ["NewCronSchedule", "CronScheduleId", "TeamName", "PhaseId", "HookRequest", "HookCaller", "HookAction", "StopReply"]
response_types = ["CronSchedule", "Vec<CronSchedule>"]
error_types = ["AtmError"]

[testing]
allowed_test_double_paths = ["atm_runtime_test_support::InMemoryCronScheduleStore", "atm_runtime_test_support::DummyHookRunner", "atm_runtime_test_support::RecordingHookActionDispatcher", "atm_cron::TokioClock", "atm_cron::SeqJitter"]
forbidden_test_bypasses = ["tokio::process::Command", "rusqlite::Connection"]

[enforcement]
lint_rules = ["LINT-BOUNDARY-CRON-SCHEDULER"]
review_gates = ["no_runtime_or_bootstrap_edge", "parse_before_write", "stop_check_before_every_run", "missed_tick_replay", "concurrent_runs_of_one_schedule"]

[status]
state = "active"
notes = ["Phase bk sprint bk-4 (2026-10): first version."]
```

## Authoritative deliverables

1. The crate and its workspace entries, dependencies exactly the
   allowlist, `[features] test-utils = ["atm-core/test-utils"]`.
   `tokio-cron-scheduler` is not adopted.
2. `expr::parse` per D-g and `AtmCron` per the contract: parse and the
   `until` check and the `jitter_s` assignment before any store write;
   reload bump only after the store call returned `Ok`.
3. The worker per `design.md` §5 rules: `next` from `clock.now()` after
   startup, each run and each reload, plus the row's `jitter_s` (no replay); one run in flight per
   schedule; sleep slices of `MAX_SLEEP_SLICE`; reload merges by id; a
   loaded row with no future occurrence ends; a panicking run is caught.
4. `run_one` per the four steps above.
5. The manifest, the lint-config entry, the four manifest edits, the
   eight forbidden edges and the release entries.
6. Tests, same unit, all `#[tokio::test(start_paused = true)]` with
   `InMemoryCronScheduleStore`, `DummyHookRunner`,
   `RecordingHookActionDispatcher` and `TokioClock`, so every time
   step is an explicit `tokio::time::advance`.

## Authoritative acceptance criteria

`crates/atm-cron/tests/expr.rs`:

- [ ] (D-g, ADR-069 D1) `boundary:BOUNDARY-CronScheduler` `cron_parse_accepts_six_seven_and_macros` (`0 * * * * *`, `0 * * * * * *`, `@daily`, `@hourly`), `cron_parse_refuses_five_fields_with_named_form` (`* * * * *`, message holds `6 or 7 fields`), `cron_parse_refuses_garbage_and_out_of_range` (`x y`, `0 0 25 * * *`, each `ATM_CRON_EXPR_INVALID` holding the parser text), `cron_parse_refuses_no_future_occurrence` (`0 0 0 1 1 * 2020`).

`crates/atm-cron/tests/worker.rs`:

- [ ] (ADR-069 D1) `boundary:BOUNDARY-CronScheduler` `start_refuses_invalid_expression_before_store_write` (store write counter 0); `start_refuses_until_in_the_past`; `start_after_end_updates_in_place_and_keeps_the_id`; `start_returns_cron_expr_byte_identical`.
- [ ] (ANC-023) `boundary:BOUNDARY-CronScheduler` `start_requires_one_stop_condition_by_type`: `CronScheduler::start` takes `NewCronSchedule`, whose `stop: StopCondition` is not an `Option`, so a schedule with none or both is unrepresentable on this path; the stored row returned by `start` carries the same `stop`. The CLI usage error is bk-5's and the CHECK refusal bk-1's.
- [ ] (ANC-020) `boundary:BOUNDARY-CronScheduler` `worker_schedules_only_active_rows`: two active and one ended row give two slots, each `next == Schedule::after(&now).next() + jitter_s`; `initial_load_error_is_returned_by_start_worker`.
- [ ] (ADR-069 D2) `boundary:BOUNDARY-CronScheduler` `start_while_running_ticks_without_restart`, `end_stops_its_ticks`, `reload_keeps_in_flight_of_a_still_active_slot`.
- [ ] (REQ-BK-D4A) `boundary:BOUNDARY-CronScheduler` `missed_ticks_are_not_replayed_after_a_pause`: every-minute schedule, `advance(180 min)` in one step, one runner call, `next` is the first minute after the new now; `wall_clock_jump_is_seen_within_one_slice`.
- [ ] (REQ-BK-D4B) `boundary:BOUNDARY-CronScheduler` `tick_while_in_flight_is_skipped_and_logged`: the `DummyHookRunner` reply is held by a `tokio::sync::Notify` across two due times; one call and one `skipped_in_flight` entry.
- [ ] (ANC-003, ANC-017) `boundary:BOUNDARY-CronScheduler` `tick_request_has_the_seven_keys_and_args_in_request_only`: the `HookRequest` in `DummyHookRunner.calls[0]` has exactly the keys `schema_version, hook, team, phase, caller, request, custom`, the same set as every task hook; `hook == "cron.tick"`, `request == {"args":["status","--json"],"created_by":<created_by>}`, `custom` equal to the row's, `caller == {identity: created_by, team, host: AtmCronDeps.host}`; the registration carries the row's `module_path`, `cwd`, `timeout_ms`.
- [ ] (ANC-022) `boundary:BOUNDARY-CronScheduler` `custom_from_schedule_reaches_cron_tick` (and the `cron.stop` request); `two_schedules_same_module_different_custom_get_their_own`: two schedules on one `module_path` with `custom` `{"goal":"a"}` and `{"goal":"b"}` and different `created_by` each tick with their own `custom` and `request.created_by`.
- [ ] (Rand 2026-10-18 cron timeout) `boundary:BOUNDARY-CronScheduler` `cron_tick_uses_cron_timeout_ms_not_timeout_ms`: a row with `timeout_ms = 20000` gives runner and stop-check deadlines of `now + 20 000 ms + HOOK_ACTION_RESERVE_MS`, above `HOOK_TIMEOUT_MAX_MS`; each action's deadline stays the dispatcher's `SERVER_REQUEST_BUDGET`.
- [ ] (Rand 2026-10-18 jitter) `boundary:BOUNDARY-CronScheduler` `jitter_is_within_half_period_capped_at_30min` (`SeqJitter` records `n == 30` for `0 * * * * *`, `1800` for `@hourly` and `@daily`; `jitter_s < n`); `jitter_avoids_offsets_held_by_same_expression` (with J = 30 and 29 offsets held, every draw gives the one free offset; with all 30 held the draw is over [0, 30); a different `cron_expr` holds nothing); `jitter_persists_across_restart` (a second `AtmCron` on the same store loads the row with the same `jitter_s` and `SeqJitter.calls` empty); `fires_at_occurrence_plus_jitter` (`@hourly`, `jitter_s = 600`: no runner call at :00, one at :10; `Until { at: :05 }` ends at :05 without a run).
- [ ] (ANC-004) `boundary:BOUNDARY-CronScheduler` `refused_records_and_dispatches_nothing`: a `{"result": "refused", "message": "quota"}` reply (`Refused { message: "quota" }`) is the tick's refusal: nothing runs, `RecordingHookActionDispatcher.calls` is empty, the only store write is `record_run` with `last_outcome == "refused: quota"`, and the `tick` log entry carries outcome `refused`; the schedule stays active. `pass_through_and_not_applicable_record_ok`: each reply gives `last_outcome == "ok"` and no dispatch.
- [ ] (ANC-006, ANC-009, ANC-021) `boundary:BOUNDARY-CronScheduler` `three_actions_dispatch_once_in_order_as_created_by`: a `Handled` reply of three actions reaches `dispatch_actions` once, through the same `HookActionDispatcher` trait the task filter calls, with the three actions in array order and unchanged (an action's own `as` is kept), `default_as == created_by` and `deadline == None`; the row records `actions: 3`.
- [ ] (ANC-006) `boundary:BOUNDARY-CronScheduler` `dispatcher_error_records_action_err`: a dispatcher with `fail_at == Some(1)` gives `last_outcome == "action_err: <code>"`, and `dispatch_actions` was called once (the dispatcher stops the list).
- [ ] (ANC-012, REQ-BK-ERRORS) `boundary:BOUNDARY-CronScheduler` `runner_errors_record_and_dispatch_nothing`: `Err(ATM_HOOK_EXIT)`, `Err(ATM_HOOK_TIMEOUT)` and `Err(ATM_HOOK_MALFORMED)` from the runner record `exit`, `timeout` and `malformed`, the dispatcher records no call, and the deadline passed to the runner is `now + timeout_ms + HOOK_ACTION_RESERVE_MS` of the row; `record_run_writes_each_outcome_class` covers every `CronRunOutcome` variant.
- [ ] (Rand panic ruling) `boundary:BOUNDARY-CronScheduler` `panicking_run_records_exit_and_worker_keeps_ticking`: a `DummyHookRunner` with `panic_on_run` records `exit` and the next due time runs again.
- [ ] `boundary:BOUNDARY-CronScheduler` `record_run_after_end_mid_run_writes_nothing`; `loaded_row_without_future_occurrence_is_ended`.
- [ ] (ANC-025) `boundary:BOUNDARY-CronScheduler` `until_reached_ends_the_schedule_without_a_run`: `Until { at: now + 90 s }` on an every-minute schedule runs once at 60 s, ends at 90 s with `active == false`, `ended_at == now + 90 s`, `last_outcome == "stopped: until reached"`, and makes no call at 120 s.
- [ ] (ANC-024) `boundary:BOUNDARY-CronScheduler` `stop_hook_true_ends_the_schedule_with_its_reason`: `stop_replies` yields `{"stop": true, "reason": "phase bead closed"}`; the row has `last_outcome == "stopped: phase bead closed"`, `active == false`, and `DummyHookRunner` saw one `cron.stop` request and no `cron.tick`. `stop_hook_false_runs_the_tick`: one `cron.stop` then one `cron.tick`. `stop_hook_malformed_skips_the_tick_and_keeps_the_row_active`: `Err(ATM_HOOK_MALFORMED)` leaves `active == true`, records `stop_check_failed: ATM_HOOK_MALFORMED`, logs a `stop_check` entry, and no `cron.tick` runs.
- [ ] `boundary:BOUNDARY-CronScheduler` `run_now_rules`: unknown id, ended row and in-flight row give the three results of the contract; otherwise the recorded string.
- [ ] `boundary:BOUNDARY-CronScheduler` `shutdown_joins_within_deadline_with_run_in_flight`.
- [ ] (REQ-BK-LOG) `boundary:BOUNDARY-CronScheduler` `tick_and_stop_check_log_entries`: a `tracing_subscriber::Layer` capture sees one `atm_cron` `tick` entry per tick and one `stop_check` entry per `Hook` stop check, each with `team`, `phase`, `hook`, `schedule`, `outcome`, `elapsed_ms`; per-action entries are the dispatcher's (bk-3).
- [ ] (REQ-BK-D3) `boundary:BOUNDARY-CronScheduler` `rg -n 'process::Command|HookRunner for' crates/atm-cron/src` is empty (the crate receives the one daemon runner); `cargo tree -p atm-cron -e normal` lists no `atm-storage`, `atm-http-runtime`, `atm-daemon-bootstrap`, `atm`, `atm-storage-rusqlite`, `atm-herdr`, `atm-graft` or `tokio-cron-scheduler`.
- [ ] `boundary:BOUNDARY-CronScheduler` `python3 -m unittest .just/tests/test_release_targets_manifest.py` passes with the renumbered release entries.

## Authoritative validation

- `cargo test -p atm-cron --features test-utils`
- `cargo clippy -p atm-cron --all-targets --features test-utils -- -D warnings`
- `cargo build --workspace`
- `just lint`

## Non-closure

- Composition (`AtmCron::new`, `install_cron_scheduler`, `start_worker`,
  the shutdown step) and the CLI are bk-5's; the routes are bk-3's. A live
  tick through `DaemonHookRunner` is bk-5's `fixture_cron_receives_args`.
