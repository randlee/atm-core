# Sprint bk-4 — atm-cron crate

closure_type `boundary`; target_boundary `BOUNDARY-CronScheduler` (the
`atm-cron` crate behind bk-2's sealed `atm_core::boundary::CronScheduler`
trait); branch `sprint/bk-4-atm-cron`, PR → `sprint/bk-7-hook-filter`
(layer 6). Wave 3, must_follow bk-3 (Rand, 2026-10-19: "do 4 waves"; overrides
`atm-beads-plan-guidelines.md:177`): cut from bk-3's final head, building
on its root `Cargo.toml`, `Cargo.lock` and `.just/lint-config.toml` edits,
and rebased onto layer 5 in order. It
consumes through `atm_core::boundary` bk-2's `CronScheduler`,
`CronStartSpec`, `HookRunner`, `HookActionDispatcher`, `DispatchSpec`,
`HookRequest::for_cron`, the doubles, and bk-1's `CronScheduleStore`
(`generation`, `created_at`, the guarded `record_run`/`record_stop`) and
`InMemoryCronScheduleStore` (`writes`). Governing text: `design.md` §5
(D-4, D-g, D-i, D-r, D-9), §9.7; `REQ-P-CRON-001` §24.1, `REQ-P-CRON-002`
§24.2; ADR-069 D1, D2, D3, D4, ADR-068 D2, ADR-065, ADR-067.

## Scope boundary

This sprint owns one new workspace crate, `crates/atm-cron`: the only
implementation of `CronScheduler` and the worker that turns active
`cron_schedule` rows into hook runs. It composes nothing: the bootstrap
(bk-5) builds `AtmCron` and installs it in the router. It adds no
storage, protocol, CLI, router or doctor code.

Owned paths (new unless marked edited):

- `crates/atm-cron/Cargo.toml`, `crates/atm-cron/src/lib.rs`,
  `crates/atm-cron/src/scheduler.rs` (`AtmCron`, the trait impl, `store_call`),
  `crates/atm-cron/src/worker.rs` (the loop, `Slot`),
  `crates/atm-cron/src/clock.rs` (`Clock`, `SystemClock`,
  `system_sleep_step`, and `ManualClock` behind `test-utils`),
  `crates/atm-cron/src/jitter.rs` (`JitterSource`, `SystemJitter`,
  `SeqJitter` behind `test-utils`, `assign`),
  `crates/atm-cron/src/tick.rs` (one run, `CronRunOutcome`),
  `crates/atm-cron/src/expr.rs` (the grammar);
  `crates/atm-cron/tests/worker.rs`,
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
  "getrandom", "serde_json", "tokio", "tracing"]` and
  `tracing-subscriber` in the crate's dev allowlist; the `cron` ownership
  entry and the `atm-runtime-test-support` section rule are bk-2's);
  `Cargo.lock`. bk-2's core
  manifests already list `atm-cron` as a dependent.
- `release/sc-publish-consumer-input.json` (edited: an `atm-cron` crate
  entry with `publish_order` 12, `required` true, `publish` true;
  `atm-daemon-bootstrap`, `atm-daemon`, `atm-graft`, `agent-team-mail`
  renumbered 13..16) and `release/publish-artifacts.toml` (re-rendered
  from its `.j2` template, the same edit)

## Public contract

```rust
// crates/atm-cron/src/lib.rs
#![forbid(unsafe_code)]
mod clock; mod expr; mod jitter; mod scheduler; mod tick; mod worker;
pub use clock::{Clock, SystemClock}; pub use jitter::{JitterSource, SystemJitter};
#[cfg(feature = "test-utils")] pub use {clock::ManualClock, jitter::SeqJitter};
pub use scheduler::{AtmCron, AtmCronDeps, CronWorkerHandle};

// crates/atm-cron/src/clock.rs
#[async_trait]
pub trait Clock: Send + Sync { fn now(&self) -> DateTime<Utc>; async fn sleep_until(&self, t: DateTime<Utc>); }   // returns once now() >= t
pub struct SystemClock;   // Utc::now(); sleep_until loops tokio::time::sleep(system_sleep_step(Utc::now(), t)), so a wall-clock jump is seen within a slice
pub(crate) const MAX_SLEEP_SLICE: Duration = Duration::from_secs(30);
pub(crate) fn system_sleep_step(now: DateTime<Utc>, t: DateTime<Utc>) -> Duration;   // min(MAX_SLEEP_SLICE, t - now), zero when t <= now
/// The test clock: time moves only on advance/set, so a test fires ticks without sleeping or controlling the tokio clock.
#[cfg(feature = "test-utils")] pub struct ManualClock { now: watch::Sender<DateTime<Utc>>, parks: watch::Sender<u64> }
#[cfg(feature = "test-utils")] impl ManualClock {
    pub fn new(start: DateTime<Utc>) -> Self;  pub fn advance(&self, d: chrono::Duration);  pub fn set(&self, t: DateTime<Utc>);
    pub fn parks(&self) -> u64;                                   // sleep_until entries that found now < t, re-parks included
    pub async fn wait_parks_above(&self, n: u64);                 // parks.subscribe().wait_for(|p| *p > n)
    pub async fn advance_settled(&self, d: chrono::Duration);     // let n = parks(); advance(d); wait_parks_above(n)
}
// sleep_until(t): loop { if now >= t return; parks += 1; now.changed().await }; advance/set use send_replace. The worker parks only
// at its one select!, so parks > n means it handled every effect of the advance and is waiting again: the quiescent barrier. It holds
// when no run is in flight or each in-flight run is held by before_reply; a run's outcome is read after shutdown has joined it.

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
    pub dispatcher: Arc<dyn HookActionDispatcher>,                            // bk-7's UnhookedRouter, below the filter (§9.9)
    pub clock: Arc<dyn Clock>, pub host: Option<HostName>,                    // clock: every stamp (created_at, ended_at, run and stop times)
    pub jitter: Arc<dyn JitterSource>,                                        // SystemJitter in bk-5; SeqJitter in tests
}
pub struct AtmCron { deps: AtmCronDeps, reload: watch::Sender<u64>, start_lock: tokio::sync::Mutex<()>, stopped: Arc<AtomicBool> }
// start_lock is held from the jitter draw to the store write; stopped is set by a drop guard in the worker task (shutdown or panic)
impl AtmCron {
    pub fn new(deps: AtmCronDeps) -> Self;
    /// Loads active_all() once (Err aborts startup in bk-5), spawns the loop, returns the handle.
    pub async fn start_worker(self: &Arc<Self>) -> Result<CronWorkerHandle, AtmError>;
}
async fn store_call<T: Send + 'static>(store: Arc<dyn CronScheduleStore>, f: impl FnOnce(&dyn CronScheduleStore) -> Result<T, AtmError> + Send + 'static) -> Result<T, AtmError>;   // tokio::task::spawn_blocking
impl atm_core::boundary::sealed::Sealed for AtmCron {}
#[async_trait] impl CronScheduler for AtmCron {
    /// stopped: daemon_unavailable("cron worker stopped") for start, end and list alike, so `atm cron list` shows it. expr::parse;
    /// stop_due(Until{at}, now): cron_expr_invalid("until is not in the future") (the timeout cap is the route's, bk-7); under start_lock:
    /// jitter_s = jitter::assign(.., held = jitter_s of active rows with the same cron_expr, other than this key); NewCronSchedule from spec,
    /// jitter_s and created_at = clock.now() (the only place one is built); store.start (D-i); then reload bump.
    async fn start(&self, spec: CronStartSpec) -> Result<CronSchedule, AtmError>;
    async fn end(&self, team: &TeamName, phase: &PhaseId, name: Option<&str>) -> Result<Vec<CronSchedule>, AtmError>;   // store.end(.., clock.now()), bump
    async fn list(&self, team: Option<&TeamName>, phase: Option<&PhaseId>) -> Result<Vec<CronSchedule>, AtmError>;
}
pub struct CronWorkerHandle { shutdown: Mutex<Option<oneshot::Sender<()>>>, worker: Mutex<Option<JoinHandle<()>>> }
impl CronWorkerHandle { pub async fn shutdown(&self, deadline: tokio::time::Instant); }   // stop scheduling, await in-flight runs until deadline, abort after

// crates/atm-cron/src/worker.rs  (pub(crate))
struct Slot { schedule: CronSchedule, next: Option<DateTime<Utc>>, in_flight: Option<JoinHandle<CronRunOutcome>> }
// next = schedule.after(&(now - Duration::seconds(jitter_s))).next().map(|o| o + Duration::seconds(jitter_s)); wake = min(next, until_at) for Until.
// loop: select! { biased; reload.changed() => merge active_all(), clock.sleep_until(plan.wake or MAX_UTC) => apply plan_step, finished
// in_flight => .., shutdown => break }. The merge keeps a slot only if the row equals its row on id, generation, cron_expr, jitter_s, stop,
// module_path, cwd, args, timeout_ms, custom; else the slot is dropped, its in_flight aborted and a new slot built. A stale run that
// outlives the abort (its record in spawn_blocking) writes nothing: record_run/record_stop carry the run's generation (bk-1 WHERE clause).
// biased order: a reload committed before a clock step is merged first; a due run starts before a shutdown sent after the step.
// Errors: active_all on reload fails -> log reload_failed, keep the slots, retry at the next wake or bump; record_* fails -> log
// record_failed. A finished in_flight: Ok(Stopped) drops the slot; JoinError is_panic -> record_run(id, generation, now, "exit: hook
// runner panicked: <payload>") and the worker keeps running (Rand panic ruling); is_cancelled -> log cancelled, record nothing.
// A loaded row with no future occurrence: record_stop(id, generation, now, "no future occurrence").
pub(crate) struct Step { pub run: Vec<CronScheduleId>, pub skip_in_flight: Vec<CronScheduleId>, pub stop_until: Vec<CronScheduleId>, pub wake: Option<DateTime<Utc>> }
/// Pure: due = next <= now; a due slot with in_flight is skip_in_flight (next recomputed); stop_due(Until) is stop_until; wake = min over
/// slots of next and until_at. The worker applies a Step and does nothing else on a clock wake.
pub(crate) fn plan_step(slots: &[SlotView], now: DateTime<Utc>) -> Step;

// crates/atm-cron/src/tick.rs  (pub(crate))
enum CronRunOutcome { Ok, Refused(String), Actions(usize), SkippedInFlight, Exit(String), Timeout, Malformed, ActionErr(AtmErrorCode), Stopped(String), StopCheckFailed(AtmErrorCode) }
// as_str: "ok" | "refused: <msg>" | "actions: <n>" | "skipped_in_flight" | "exit" | "timeout" | "malformed" | "action_err: <code>" | "stopped: <reason>" | "stop_check_failed: <code>"
pub(crate) async fn run_one(cron: &AtmCron, schedule: &CronSchedule) -> CronRunOutcome;
/// The one Until decision, used by start, plan_step and run_one: Until { at } with now >= at.
pub(crate) fn stop_due(stop: &StopCondition, now: DateTime<Utc>) -> bool;
```

`run_one` runs these steps in order.

1. **Stop check.**
   - `stop_due` calls `record_stop(id, generation, now, "until reached")`
     and returns `Stopped`.
   - `Hook` calls `runner.stop(&HookTarget::from(schedule), ..)` with
     `HookRequest::for_cron(target, caller, cron.stop, &args)` under
     `RequestDeadline::after(timeout_ms + HOOK_ACTION_RESERVE_MS)`, so the
     runner's `runner_budget` gives the row's `timeout_ms` (up to
     `CRON_TIMEOUT_MAX_MS`) rather than the task-hook cap.
     `{"stop": true, "reason": r}` gives `record_stop(id, generation, ..)`
     and `Stopped(r)`.
     `{"stop": false}` continues to step 2. An `Err` (`ATM_HOOK_EXIT`,
     `ATM_HOOK_TIMEOUT`, `ATM_HOOK_MALFORMED`) gives
     `StopCheckFailed(code)`: no tick runs and the row stays active.
2. **Run.** Call `runner.run` with the same `HookTarget` and
   `HookRequest::for_cron(target, caller, cron.tick, &args)`.
   - The caller is `HookCaller { identity: created_by, team, host }`.
   - Its deadline is the same as the stop check's: the row's `timeout_ms`,
     never the module's task-hook cap.
   - The request's `custom` is the row's `custom` and `request` is
     `{"args": [...], "created_by": <created_by>}` (bk-2 `for_cron`).
3. **Reply.**
   - `Refused { message }` stops the tick before any action: `Refused`,
     whose message is the recorded outcome; nothing is dispatched.
   - `PassThrough` and `NotApplicable` give `Ok` (nothing to do).
   - `Handled(list)` (at most `HOOK_ACTIONS_MAX` = 32, bk-2's parser)
     calls `dispatcher.dispatch_actions(DispatchSpec { hook: cron.tick,
     team, phase, default_as: created_by, deadline: None, dry_run: false },
     list)`. `None` gives each action `now + SERVER_REQUEST_BUDGET` (D-q),
     so one run is bounded by 32 action budgets. The result is
     `Actions(n)` or `ActionErr(code)`.
   - A runner `Err` fails the tick closed: it maps by its code to `Exit`,
     `Timeout` or `Malformed`, and nothing is dispatched. The runner killed
     the child on timeout, at most `CRON_TIMEOUT_MAX_MS` (30 000 ms).
4. **Record.** Call `record_run(id, generation, now, outcome.as_str())`
   with the generation of the row the run started from. `false` means the
   row was ended or restarted mid-run: the outcome is logged and nothing is
   written.

Log entries (ADR-067, `tracing::info!`): `subsystem = "atm_cron"`, `action
= "stop_check" | "tick"`, `outcome` as `as_str` without its suffix, plus
`team`, `phase`, `hook`, `schedule`, `elapsed_ms`, and `reason` on
`stopped`; `reload_failed`, `record_failed` and `cancelled` entries carry
the error. Each tick action gets its one `atm_hook` `action` entry from
bk-7's `dispatch_actions`.

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
notes = "The only CronScheduler implementation (Rand 2026-10-18, PR #1877: own crate, simple trait)."

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
allowed_dev_dependencies = ["atm-core", "atm-runtime-test-support", "tokio", "tracing-subscriber"]
forbidden_edges = []   # allowed_dependents and allowed_dependencies are the guard (lint_boundaries.py)

[references]
scope = "outside_owner_crate"
forbidden = []

[contracts]
request_types = ["CronStartSpec", "CronScheduleId", "TeamName", "PhaseId", "HookRequest", "HookCaller", "HookAction", "StopReply"]
response_types = ["CronSchedule", "Vec<CronSchedule>"]
error_types = ["AtmError"]

[testing]
allowed_test_double_paths = []
forbidden_test_bypasses = ["tokio::process::Command", "rusqlite::Connection"]

[enforcement]
lint_rules = ["LINT-BOUNDARY-CRON-SCHEDULER"]
review_gates = ["no_runtime_or_bootstrap_edge", "parse_before_write", "stop_check_before_every_run", "missed_tick_replay", "concurrent_runs_of_one_schedule"]
io_forbidden_source_modules = { sqlite = ["atm_cron::scheduler", "atm_cron::worker", "atm_cron::tick", "atm_cron::clock", "atm_cron::jitter", "atm_cron::expr"], process_spawn = ["atm_cron::scheduler", "atm_cron::worker", "atm_cron::tick", "atm_cron::clock", "atm_cron::jitter", "atm_cron::expr"], http_server = ["atm_cron::scheduler", "atm_cron::worker", "atm_cron::tick", "atm_cron::clock", "atm_cron::jitter", "atm_cron::expr"] }

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
   startup, each run and each reload, by the jitter formula above (no
   replay); `plan_step` and `stop_due`; one run in flight per schedule;
   sleep slices of `MAX_SLEEP_SLICE` in `SystemClock`; the reload merge on
   `generation` and the row fields; the error and `JoinError` rules above;
   the `stopped` drop guard.
4. `run_one` per the four steps above.
5. The manifest, the lint-config entries and the release entries.
6. Tests, same unit, plain `#[tokio::test]` with
   `InMemoryCronScheduleStore`, `DummyHookRunner`,
   `RecordingHookActionDispatcher` and `ManualClock`. A tick fires only on
   `ManualClock::advance` or `set`; between steps a test waits on
   `advance_settled` (or `wait_parks_above` after a `start`), a held call
   through `DummyHookRunner.before_reply` on a tokio `watch`, and reads rows
   and counts after `CronWorkerHandle::shutdown` has joined the in-flight
   runs. Timing rules (what runs, skips or stops at a given now) are
   asserted on the pure `plan_step`. No test sleeps or controls the tokio
   clock.

## Authoritative acceptance criteria

`crates/atm-cron/tests/expr.rs`:

- [ ] (D-g, ADR-069 D1) `boundary:BOUNDARY-CronScheduler` `cron_parse_accepts_six_seven_and_macros` (`0 * * * * *`, `0 * * * * * *`, `@daily`, `@hourly`); `cron_parse_refuses_five_fields_with_named_form` (message holds `6 or 7 fields`); `cron_parse_refuses_garbage_and_out_of_range` (`x y`, `0 0 25 * * *`: `ATM_CRON_EXPR_INVALID` with the parser text); `cron_parse_refuses_no_future_occurrence` (`0 0 0 1 1 * 2020`).

`crates/atm-cron/tests/worker.rs` unless stated:

- [ ] (ADR-069 D1) `boundary:BOUNDARY-CronScheduler` `start_refuses_invalid_expression_before_store_write` (`writes == 0`); `start_refuses_until_in_the_past`; `start_after_end_updates_in_place_and_keeps_the_id`; `start_returns_cron_expr_byte_identical`; `start_stamps_created_at_from_the_injected_clock`.
- [ ] (ANC-023) `boundary:BOUNDARY-CronScheduler` `start_requires_one_stop_condition_by_type`: `CronStartSpec.stop` is a `StopCondition`, not an `Option`; the row carries the same `stop`.
- [ ] (ANC-020) `boundary:BOUNDARY-CronScheduler` `worker_schedules_only_active_rows` (two active, one ended: two slots, `next == after(now - jitter_s) + jitter_s`); `initial_load_error_is_returned_by_start_worker`; `restart_inside_jitter_window_still_fires_that_occurrence` (`@hourly`, `jitter_s = 600`, started at 10:00:30: one call by 10:10).
- [ ] (ADR-069 D2, D-i) `boundary:BOUNDARY-CronScheduler` `start_while_running_ticks_without_restart`, `end_stops_its_ticks`, `reload_keeps_in_flight_of_an_unchanged_slot`, `end_then_start_before_reload_applies_new_expression_and_jitter`; `stale_run_never_writes_the_restarted_row` (a run of the generation-1 row, held by `before_reply` across `end` and `start`, completes; after shutdown the generation-2 row has `last_outcome == None` and `writes` counts only the end and start); `restart_during_tick_does_not_replay_after_reload`; `concurrent_starts_same_expression_get_distinct_jitter`.
- [ ] (REQ-BK-D4A) `boundary:BOUNDARY-CronScheduler` `missed_ticks_are_not_replayed_after_a_pause` (every minute, one `advance_settled(180 min)`: one call, `next` the first minute after the new now); `clock.rs::tests::system_sleep_step_is_capped_at_the_slice` (pure: 1 h ahead → 30 s, 10 s → 10 s, past → zero).
- [ ] (REQ-BK-D4B) `boundary:BOUNDARY-CronScheduler` `tick_while_in_flight_is_skipped_and_logged` (reply held by `before_reply` across two `advance_settled(60 s)`; after release and shutdown one call, two `skipped_in_flight` entries); `worker.rs::tests::plan_step_skips_a_due_slot_in_flight` (pure).
- [ ] (ANC-003, ANC-017, ANC-022) `boundary:BOUNDARY-CronScheduler` `tick_request_has_the_seven_keys_and_args_in_request_only`: `calls[0]` has the task-hook keys, `hook == "cron.tick"`, `request == {"args":["status","--json"],"created_by":<created_by>}`, `custom` byte-equal to the row's (also on `cron.stop`), `caller == {created_by, team, AtmCronDeps.host}`, the target's `module_path`, `cwd`, `timeout_ms`; `two_schedules_same_module_different_custom_get_their_own`.
- [ ] (Rand 2026-10-18 cron timeout) `boundary:BOUNDARY-CronScheduler` `cron_tick_uses_cron_timeout_ms_not_timeout_ms`: `timeout_ms = 20000` gives stop-check and tick calls with that `timeout_ms` and `remaining_ms()` above `HOOK_TIMEOUT_MAX_MS` and at most 20 500; `DispatchCall.spec.deadline == None`.
- [ ] (Rand 2026-10-18 jitter) `boundary:BOUNDARY-CronScheduler` `jitter_is_within_half_period_capped_at_30min` (`SeqJitter` sees `n == 30` for `0 * * * * *`, 1800 for `@hourly` and `@daily`); `jitter_avoids_offsets_held_by_same_expression` (J = 30: 29 held gives the free one; 30 held draws over [0, 30); another expression holds nothing); `jitter_persists_across_restart`; `worker.rs::tests::plan_step_fires_at_occurrence_plus_jitter` (pure, `@hourly`, `jitter_s = 600`: no `run` at :00, `run` and `wake == :10` at :10; `Until { at: :05 }` gives `stop_until` at :05, no `run`); `worker_fires_at_occurrence_plus_jitter` (`advance_settled` to :00 and :10, after shutdown one call).
- [ ] (ANC-004) `boundary:BOUNDARY-CronScheduler` `refused_records_and_dispatches_nothing` (`Refused{"quota"}`: no `DispatchCall`, `last_outcome == "refused: quota"`, still active); `pass_through_and_not_applicable_record_ok`.
- [ ] (ANC-006, ANC-009, ANC-021) `boundary:BOUNDARY-CronScheduler` `three_actions_dispatch_once_in_order_as_created_by` (one `DispatchCall`, actions unchanged, `spec.default_as == created_by`, `spec.deadline == None`, `spec.dry_run == false`, `actions: 3` recorded); actions go only to `deps.dispatcher` (bk-7's `UnhookedRouter`, ANC-008), never a `TaskCommands`; `dispatcher_error_records_action_err` (`fail_at == Some(1)`: `action_err: <code>`, one call).
- [ ] (ANC-012, REQ-BK-ERRORS) `boundary:BOUNDARY-CronScheduler` `runner_errors_record_and_dispatch_nothing` (`Err` `ATM_HOOK_EXIT`, `ATM_HOOK_TIMEOUT`, `ATM_HOOK_MALFORMED` record `exit`, `timeout`, `malformed`, no dispatch, deadline `remaining_ms()` at most `timeout_ms + HOOK_ACTION_RESERVE_MS`); `record_run_writes_each_outcome_class` (every `CronRunOutcome`).
- [ ] (Rand panic ruling) `boundary:BOUNDARY-CronScheduler` `panicking_run_records_exit_and_worker_keeps_ticking` (`exit: hook runner panicked: `, the next due time runs); `cancelled_run_records_nothing` (a reload aborting a held run: outcome unchanged, one `cancelled` entry).
- [ ] (store errors) `boundary:BOUNDARY-CronScheduler` `reload_failure_keeps_slots_and_retries` (`fail_active_all` on a bump: `reload_failed`, the old slot fires; cleared, the next bump merges); `record_failure_is_logged` (`fail_writes`: `record_failed`, ticking continues); `stopped_worker_refuses_start_end_and_list` (after shutdown: `cron worker stopped`); `record_run_after_end_mid_run_writes_nothing`; `loaded_row_without_future_occurrence_is_ended`.
- [ ] (ANC-025) `boundary:BOUNDARY-CronScheduler` `until_reached_ends_the_schedule_without_a_run`: `Until { at: now + 90 s }`, every minute: `plan_step` gives `run` at 60 s, `stop_until` at 90 s, nothing at 120 s; driven by `advance_settled` to 60, 90, 120 s, after shutdown one call, `active == false`, `ended_at == now + 90 s`, `stopped: until reached`; `tick.rs::tests::stop_due_is_the_one_until_rule` (`at - 1 s` false, `at` true).
- [ ] (ANC-024) `boundary:BOUNDARY-CronScheduler` `stop_hook_true_ends_the_schedule_with_its_reason` (`stopped: phase bead closed`, inactive, one `cron.stop`, no `cron.tick`); `stop_hook_false_runs_the_tick`; `stop_hook_failure_skips_the_tick_and_keeps_the_row_active` (each runner `Err`: active, `stop_check_failed: <code>`, no `cron.tick`).
- [ ] `boundary:BOUNDARY-CronScheduler` `ticks_fire_only_when_the_manual_clock_reaches_them` (no call before occurrence + `jitter_s`, then one; an ended row's occurrence gives none); `shutdown_joins_within_deadline_with_run_in_flight`; `AtmCron` has no public method besides `new`, `start_worker` and the trait's.
- [ ] (REQ-BK-LOG) `boundary:BOUNDARY-CronScheduler` `tick_and_stop_check_log_entries`: one `atm_cron` `tick` entry per tick and one `stop_check` per `Hook` check, each with `team`, `phase`, `hook`, `schedule`, `outcome`, `elapsed_ms`; action entries come from bk-7's dispatcher, not here.
- [ ] (REQ-BK-D3) `boundary:BOUNDARY-CronScheduler` `rg -n 'process::Command|HookRunner for' crates/atm-cron/src` is empty.
- [ ] `boundary:BOUNDARY-CronScheduler` `python3 -m unittest .just/tests/test_release_targets_manifest.py` passes with the renumbered entries.

## Authoritative validation

- `cargo test -p atm-cron --features test-utils`
- `cargo clippy -p atm-cron --all-targets --features test-utils -- -D warnings`
- `cargo build --workspace`
- `cargo test -p atm-architecture --test hook_trait_boundary` (now visiting `atm-cron`)
- `just lint` (boundaries, `.just/check_line_counts.py`)

## Non-closure

- Composition (`AtmCron::new`, `install_cron_scheduler`, `start_worker`,
  the shutdown step) and the CLI are bk-5's; the routes are bk-7's. A live
  tick through `DaemonHookRunner` is bk-5's
  `fixture_cron_receives_args_through_the_composed_scheduler`, on `ManualClock`.
