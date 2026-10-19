# Sprint bk-4 — atm-cron crate

closure_type `boundary`; target_boundary `BOUNDARY-CronScheduler` (the
`atm-cron` crate behind bk-2's sealed `atm_core::boundary::CronScheduler`
trait); branch `sprint/bk-4-atm-cron`, PR → the stack top above bk-2
(`sprint/bk-2-hook-contract`); difficulty normal. Develops in
parallel with bk-3 (disjoint owned paths), rebased onto the stack after
bk-3. Consumes bk-1 (the store, row types, `StopCondition`) and bk-2 (the
hook types, the sealed `HookRunner`, `CronScheduler`,
`HookActionDispatcher` traits through `atm_core::boundary`,
`AtmErrorCode::CronExprInvalid`, the doubles in
`atm-runtime-test-support`) only. Governing text: `design.md` §5, D-4
(Rand's own-crate ruling), D-9. Requirements `REQ-P-CRON-002` §24.2,
`REQ-P-CRON-001` §24.1 (parse before write, rows persist, every active row
runs); ADR-069 D1, D2, D3, D4, ADR-068 D2 (reply handling), ADR-065,
ADR-067.

## Scope boundary

This sprint owns one new workspace crate, `crates/atm-cron`: the only
implementation of `CronScheduler` (schedule start, end, list with
parse-before-write) and the worker loop that turns active `cron_schedule`
rows into hook runs. Its dependencies are exactly the allowlist
below and never `atm-http-runtime`, `atm-daemon-bootstrap`, `atm`,
`atm-storage-rusqlite`, `atm-herdr` or `atm-graft`. It composes nothing:
the daemon bootstrap (bk-5) builds `AtmCron` and hands it to the router as
`Arc<dyn CronScheduler>`; the router (bk-3) implements
`HookActionDispatcher`, which this crate only calls. It adds no storage,
protocol, CLI, router or doctor code.

Owned paths (new unless marked edited):

- `crates/atm-cron/Cargo.toml`, `crates/atm-cron/src/lib.rs`,
  `crates/atm-cron/src/scheduler.rs` (`AtmCron`, the `CronScheduler` impl),
  `crates/atm-cron/src/worker.rs` (the loop, `Slot`, `CronRunOutcome`),
  `crates/atm-cron/src/clock.rs` (`Clock`, `SystemClock`),
  `crates/atm-cron/src/tick.rs` (one run: request build, runner call, reply
  mapping, `record_run`), `crates/atm-cron/tests/worker.rs`
- `Cargo.toml` (edited: `"crates/atm-cron"` in `[workspace] members` after
  `"crates/atm-storage-rusqlite"`; `atm-cron = { path = "crates/atm-cron" }`
  and `cron = "0.17"` in `[workspace.dependencies]`)
- `boundaries/atm-cron/cron-scheduler.toml` (new; full text below)
- `.just/lint-config.toml` (edited: one
  `[[boundaries.manifest_dependency_allowlists]]` entry for
  `crates/atm-cron/Cargo.toml` with `boundary_record_path =
  "boundaries/atm-cron/cron-scheduler.toml"` and `allowed_dependencies =
  ["async-trait", "atm-core", "atm-runtime-test-support", "atm-storage",
  "chrono", "cron", "serde_json", "tokio", "tracing"]`)
- `crates/atm-architecture/tests/boundary_enforcement.rs` (edited: the
  five forbidden edges below added to the edge table)

## Public contract

The trait this crate implements is bk-2's, repeated here so that the two
sprints are read against one text:

```rust
// crates/atm-core/src/boundary/cron_scheduler.rs  (bk-2; sealed like HookRunner)
#[async_trait]
pub trait CronScheduler: sealed::Sealed + Send + Sync {
    /// `cron::Schedule::from_str` on the 7-field seconds-first dialect; Err is AtmErrorCode::CronExprInvalid with the parser's message.
    fn validate_expression(&self, cron_expr: &str) -> Result<(), AtmError>;
    /// validate_expression, then CronScheduleStore::start (Err on an active duplicate of (team, phase, name)), then the worker reloads (ADR-069 D2).
    fn start(&self, schedule: NewCronSchedule) -> Result<CronSchedule, AtmError>;
    /// CronScheduleStore::end (rows kept, `active = false`, `ended_at` set), then the worker reloads; returns the rows ended.
    fn end(&self, team: &TeamName, phase: &PhaseId, name: Option<&str>) -> Result<Vec<CronSchedule>, AtmError>;
    fn list(&self, team: Option<&TeamName>, phase: Option<&PhaseId>) -> Result<Vec<CronSchedule>, AtmError>;
    /// Runs one schedule now exactly as a tick does (the tick/run contract); the worker loop and tests share it.
    async fn run_now(&self, id: CronScheduleId) -> Result<String, AtmError>;   // Ok(outcome string written by record_run)
}
pub struct NoopCronScheduler;   // router default until bk-5 injects AtmCron: every method Err(AtmError::validation("no cron scheduler composed"))

// crates/atm-core/src/boundary/hook_action_dispatcher.rs  (bk-2; implemented by StorageAndNudgeRouter in bk-3)
#[async_trait]
pub trait HookActionDispatcher: sealed::Sealed + Send + Sync {
    /// Dispatches `actions` in order with hooks disabled, as `default_as` unless an action names `as`, with ingress AuthenticatedIngress::Local;
    /// stops at the first Err and returns it (ANC-006); Ok(n) is the number of actions that ran.
    async fn dispatch_actions(&self, hook: &HookName, team: &TeamName, default_as: &AgentName, actions: Vec<HookAction>) -> Result<usize, AtmError>;
}
```

The crate:

```rust
// crates/atm-cron/src/lib.rs
#![forbid(unsafe_code)]
mod clock; mod scheduler; mod tick; mod worker;
pub use clock::{Clock, SystemClock};
pub use scheduler::{AtmCron, AtmCronDeps, CronWorkerHandle};
// Everything else (Slot, CronRunOutcome, tick::run_one) is pub(crate): tests reach it through run_now and the stores.

// crates/atm-cron/src/clock.rs
/// `now()` is the only clock the crate reads, so tests inject a fixed `Utc` time; production passes `SystemClock`.
pub trait Clock: Send + Sync { fn now(&self) -> DateTime<Utc>; }
pub struct SystemClock;   // Utc::now()

// crates/atm-cron/src/scheduler.rs
pub struct AtmCronDeps {
    pub store: Arc<dyn CronScheduleStore>,
    pub runner: Arc<dyn HookRunner>,      // the one HookRunner the router uses, built once in bootstrap (bk-5); this crate never constructs or spawns one (REQ-BK-D3)
    pub dispatcher: Arc<dyn HookActionDispatcher>,
    pub clock: Arc<dyn Clock>,
    pub observability: Arc<dyn ObservabilityPort + Send + Sync>,
    pub host: Option<HostName>,              // HookCaller.host of every tick and stop request; bootstrap passes AtmConfig.local_host (None when unset)
}
pub struct AtmCron { deps: AtmCronDeps, reload: tokio::sync::watch::Sender<u64> }
impl AtmCron {
    pub fn new(deps: AtmCronDeps) -> Self;
    /// Spawns the worker loop (worker.rs) on the current Tokio runtime; the handle joins it. Called once by the bootstrap after the router is built.
    pub fn start_worker(self: &Arc<Self>) -> CronWorkerHandle;
}
impl sealed::Sealed for AtmCron {}
#[async_trait] impl CronScheduler for AtmCron { /* the five methods above; start and end call self.reload.send_modify(|n| *n += 1) after the store call returns Ok */ }
pub struct CronWorkerHandle { shutdown: Mutex<Option<oneshot::Sender<()>>>, worker: Mutex<Option<JoinHandle<()>>> }
impl CronWorkerHandle {
    /// As AtmTempSweeperRuntime::shutdown: send shutdown, join under `deadline`, abort after it.
    pub async fn shutdown(&self, deadline: tokio::time::Instant);
}

// crates/atm-cron/src/worker.rs  (pub(crate))
struct Slot { schedule: CronSchedule, next: Option<DateTime<Utc>>, in_flight: Option<JoinHandle<()>> }
/// Loads store.active_all() once, builds one Slot per row with next = cron::Schedule::from_str(cron_expr)?.after(&clock.now()).next(), then loops:
/// select! { _ = sleep_until(earliest wake) => tick that slot, Ok(()) = reload.changed() => reload active_all() and rebuild every slot, _ = shutdown_rx => break }.
/// wake(slot) = min(next, until_at) when the schedule's stop is Until (D-9), else next.
/// A row whose cron_expr fails to parse at load (cannot happen after validate_expression; defensive) is logged outcome = "malformed_expr" and never scheduled.
async fn run_loop(cron: Arc<AtmCron>, reload: watch::Receiver<u64>, shutdown: oneshot::Receiver<()>);
/// One tick: if slot.in_flight is Some and not finished => log outcome = "skipped_in_flight", next = after(now); else spawn tick::run_one and set in_flight.
/// run_one performs the stop check first; when it returns Stopped the loop drops the slot.
fn tick(slot: &mut Slot, cron: &Arc<AtmCron>);

// crates/atm-cron/src/tick.rs  (pub(crate))
enum CronRunOutcome { Ok, Refused(String), Actions(usize), SkippedInFlight, Exit, Timeout, Malformed, ActionErr(AtmErrorCode), Stopped(String), StopCheckFailed(AtmErrorCode) }
impl CronRunOutcome { fn as_str(&self) -> String; }  // "ok" | "refused: <msg>" | "actions: <n>" | "skipped_in_flight" | "exit" | "timeout" | "malformed" | "action_err: <code>" | "stopped: <reason>" | "stop_check_failed: <code>"
/// Stop check first (ADR-069 D4, design D-9), before any run:
///   StopCondition::Until { at } with clock.now() >= at => store.record_stop(id, now, "until reached"), outcome Stopped("until reached"), no run;
///   StopCondition::Hook => runner.stop(reg_from(schedule, cron.stop), HookRequest { hook: cron.stop, request: {"args": schedule.args}, ..same caller }):
///     Ok(StopReply { stop: true, reason: Some(r) }) => record_stop(id, now, &r), Stopped(r), no run;  Ok(StopReply { stop: false, .. }) => continue to the run;
///     Err(e) (HookExit | HookTimeout | HookMalformed) => StopCheckFailed(e.code): the tick is skipped, record_run(id, now, "stop_check_failed: <code>"), the row stays active.
/// Then builds the tick request, runs it, maps the reply, records the row, writes the log entry; returns the outcome string it recorded.
/// Request: HookRequest with the same seven top-level keys as every task hook (ANC-003): hook = "cron.tick", team, phase,
///   caller = HookCaller { identity: schedule.created_by, team: schedule.team, host: deps.host }, request = {"args": schedule.args}, custom = {} (the field is the hook owner's; a cron schedule carries no owner custom, so it is {} and atm-cron never validates or interprets it).
///   The args travel only inside the stdin request (ADR-069 D3); the module's argv is `python3 <module_path> cron.tick`, nothing more.
/// Registration passed to the runner: HookRegistration { team, phase, hook: cron.tick (or cron.stop for the stop check), module_path, cwd, timeout_ms, active: true, registered_by: created_by, registered_at: created_at, disabled_at: None } built from the row.
/// Reply mapping: Ok(HookOutcome{reply: Refused{message}}) => Refused(message), nothing dispatched (ANC-004);
///   PassThrough => Ok; Actions(list) => deps.dispatcher.dispatch_actions(cron.tick, team, created_by, list): Ok(n) => Actions(n), Err(e) => ActionErr(e.code) (ANC-006, ANC-021);
///   Err(e) => by e.code: HookExit => Exit, HookTimeout => Timeout, HookMalformed => Malformed; nothing dispatched (REQ-BK-ERRORS).
/// Then store.record_run(id, clock.now(), outcome.as_str()) and one ADR-067 tick entry (the stop check wrote its own entry first).
async fn run_one(cron: &AtmCron, schedule: &CronSchedule) -> String;
```

Log entries (ADR-067, `tracing::info!`): `subsystem = "atm_cron"`,
`action = "stop_check"` (`hook = "cron.stop"`, one entry per `Hook` stop
check, `outcome` `continue | stopped | stop_check_failed`) and
`action = "tick"` (`hook = "cron.tick"`, `outcome` one of
`ok | refused | actions | skipped_in_flight | exit | timeout | malformed | action_err | stopped | malformed_expr`),
each with `team`, `phase`, `hook`, `schedule` (name), `elapsed_ms`, and
`reason` on `stopped`: one entry per hook call. A `cron.tick` reply's
actions go through `dispatch_actions`, and the dispatcher (bk-3, ADR-068
D7) writes the ADR-067 entry per action with the same five fields; so
every call and every action on the cron path is logged once.

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
notes = "The only CronScheduler implementation. Rand 2026-10-18 on PR #1877: atm cron is its own crate with a simple trait exposing its features; the trait is sealed in atm_core::boundary (bk-2), this crate implements it and owns the tick loop."

[implementation]
type = "AtmCron"
module = "atm_cron::scheduler"
visibility = "public"
constructor = "public"

[composition]
roots = ["atm_daemon_bootstrap::run_replacement_daemon"]

[ownership]
io_owns = ["cron_expr_parse", "schedule_next_computation", "stop_condition_check", "cron_tick", "reload_signal", "cron_run_record"]
io_forbidden = ["sqlite", "process_spawn", "http_server", "daemon_request_dispatch", "missed_tick_replay", "concurrent_runs_of_one_schedule"]

[dependencies]
allowed_dependents = ["atm-daemon-bootstrap"]
allowed_dependencies = ["atm-core", "atm-storage", "async-trait", "chrono", "cron", "serde_json", "tokio", "tracing"]
forbidden_edges = ["atm-cron -> atm-http-runtime", "atm-cron -> atm-daemon-bootstrap", "atm-cron -> atm-storage-rusqlite", "atm-cron -> atm-herdr", "atm-cron -> atm-graft", "atm-core -> atm-cron", "atm-storage -> atm-cron", "atm-http-runtime -> atm-cron"]

[references]
scope = "outside_owner_crate"
forbidden = ["AtmCron::new", "cron::Schedule"]

[contracts]
request_types = ["NewCronSchedule", "CronScheduleId", "TeamName", "PhaseId"]
response_types = ["CronSchedule", "Vec<CronSchedule>", "String (recorded outcome)"]
error_types = ["AtmError"]
notes = ["process spawning belongs to the injected HookRunner; action dispatch to the injected HookActionDispatcher; this crate owns only when a schedule runs and what is recorded."]

[testing]
allowed_test_double_paths = ["atm_runtime_test_support::InMemoryCronScheduleStore", "atm_runtime_test_support::DummyHookRunner", "atm_runtime_test_support::RecordingHookActionDispatcher"]
forbidden_test_bypasses = ["tokio::process::Command", "rusqlite::Connection"]

[enforcement]
lint_rules = ["LINT-BOUNDARY-CRON-SCHEDULER"]
review_gates = ["no_runtime_or_bootstrap_edge", "utc_next_from_now_only", "one_run_in_flight_per_schedule", "parse_before_write", "stop_check_before_every_run"]

[status]
state = "active"
notes = ["Phase bk sprint bk-4 (2026-10): first version."]
```

## Authoritative deliverables

1. The crate as above: `Cargo.toml` with `[lib] name = "atm_cron"`,
   workspace-inherited package fields,
   dependencies exactly the allowlist (dev-dependencies: `atm-core` with
   `test-utils`, `atm-runtime-test-support`); workspace `members` and
   `[workspace.dependencies]` entries; `tokio-cron-scheduler` is not adopted.
2. `AtmCron` implementing `CronScheduler`: `validate_expression` and
   `start` parse with `cron::Schedule::from_str` first and map the error to
   `AtmErrorCode::CronExprInvalid` carrying the parser's message, so the
   store never holds an invalid string (`REQ-P-CRON-001`, ADR-069 D1);
   `start` and `end` bump the reload channel only after the store call
   returned `Ok` (ADR-069 D2); `run_now` runs `tick::run_one` for the
   row and returns the string it recorded.
3. The worker loop and `CronWorkerHandle`, modeled on
   `AtmTempSweeperRuntime` (`atm_temp_sweeper_runtime.rs`: `start`,
   `shutdown`, `Drop`): `sleep_until` on the earliest `next`,
   oneshot shutdown, join under the deadline, abort after it. `next` is
   always `schedule.after(&clock.now()).next()` after startup, after each
   run and after a reload; missed occurrences are never replayed
   (REQ-BK-D4A). One in-flight run per schedule; a tick while `in_flight`
   is running is skipped and logged `skipped_in_flight` (REQ-BK-D4B).
   Rows come only from `CronScheduleStore::active_all` (ANC-020); the crate
   writes only `record_run`.
4. `tick::run_one` exactly as the contract above: the stop check first on
   every tick (D-9, ADR-069 D4; `Until` reached or `{"stop": true}` ends
   the row through `record_stop` and runs nothing, `{"stop": false}`
   proceeds, a failed stop check skips the tick, logs and records
   `stop_check_failed: <code>` and leaves the row active: ANC-023,
   ANC-024, ANC-025); then the published `HookRequest` for `cron.tick`
   with `custom` `{}` (a schedule has no owner custom; the task-path
   pass-through is bk-3's) and no key of its own (ANC-003), args only in
   the stdin request (ADR-069 D3, ANC-017), the injected `HookRunner`
   under `timeout_ms` (ANC-012), refused dispatches nothing (ANC-004),
   actions through `dispatch_actions` as `created_by` unless `as`
   (ANC-006, ANC-021), runner errors dispatch nothing (REQ-BK-ERRORS),
   then `record_run`.
5. ADR-067 entries as listed above: one per stop check, one per tick.
6. The boundary manifest above, the `.just/lint-config.toml` allowlist
   entry, and the forbidden edges added to
   `crates/atm-architecture/tests/boundary_enforcement.rs`
   (`EXPECTED_FORBIDDEN_EDGES`): `atm-cron -> atm-http-runtime`,
   `atm-cron -> atm-daemon-bootstrap`, `atm-cron -> atm-storage-rusqlite`,
   `atm-cron -> atm-herdr`, `atm-cron -> atm-graft`, `atm-core -> atm-cron`,
   `atm-storage -> atm-cron`, `atm-http-runtime -> atm-cron`.
7. Tests, same unit, in `crates/atm-cron/tests/worker.rs`, all under
   `#[tokio::test(start_paused = true)]` with `InMemoryCronScheduleStore`,
   `DummyHookRunner`, `RecordingHookActionDispatcher` and a fixed `Clock`
   (REQ-BK-TESTS): `validate_rejects_six_field_and_garbage_expressions`,
   `start_refuses_invalid_expression_before_store_write` (store call
   counter 0), `start_refuses_active_duplicate_name`,
   `end_keeps_rows_inactive_and_signals_reload`,
   `next_is_first_occurrence_after_clock_now`,
   `missed_ticks_are_not_replayed_after_a_pause` (3-hour jump, one run),
   `tick_while_in_flight_is_skipped_and_logged`,
   `tick_request_has_the_seven_keys_and_args_in_request_only`,
   `record_run_writes_each_outcome_class` (one case per `CronRunOutcome`
   variant), `actions_stop_at_first_dispatcher_error`,
   `reload_signal_loads_a_new_row` (a row inserted after start ticks after
   `start` is called for it), `ended_row_stops_ticking_after_reload`,
   `shutdown_joins_within_deadline_with_run_in_flight`,
   `run_now_returns_the_recorded_outcome`,
   `until_reached_ends_the_schedule_without_a_run`,
   `until_wake_precedes_next_occurrence`,
   `stop_hook_true_ends_the_schedule_with_its_reason`,
   `stop_hook_false_runs_the_tick`,
   `stop_hook_malformed_skips_the_tick_and_keeps_the_row_active`.

## Authoritative acceptance criteria

- [ ] (ANC-017) `boundary:CronScheduler` a schedule with `args = ["status", "--json"]` produces one `HookRequest` with `hook == "cron.tick"` and `request == {"args": ["status", "--json"]}`; `DummyHookRunner.calls[0]` carries it and the registration passed to the runner has `hook == cron.tick`, the row's `module_path`, `cwd` and `timeout_ms`; no argv beyond the hook name is implied by the request (the runner's argv is bk-3's).
- [ ] (ANC-020) `boundary:CronScheduler` `start_worker` with a store holding two active rows and one ended row schedules exactly two slots; the test asserts each slot's next time equals `cron::Schedule::from_str(expr).after(&now).next()`.
- [ ] (ANC-021, ANC-004) `boundary:CronScheduler` a `Refused("quota")` reply records `last_outcome == "refused: quota"` and the dispatcher records no call; an `Actions` reply of three actions reaches `dispatch_actions` once with the three actions in array order, `default_as == created_by`, and records `actions: 3`.
- [ ] (ANC-006) `boundary:CronScheduler` a dispatcher that fails on the second of three actions makes `last_outcome == "action_err: <code>"` and `dispatch_actions` is called once (the dispatcher, not the crate, stops the sequence).
- [ ] (REQ-BK-ERRORS, ANC-012) `boundary:CronScheduler` runner errors `HookExit`, `HookTimeout`, `HookMalformed` record `exit`, `timeout`, `malformed` and the dispatcher records no call; the registration carries the schedule's `timeout_ms`.
- [ ] (ANC-003) `boundary:CronScheduler` the `HookRequest` the runner receives has exactly the keys `schema_version, hook, team, phase, caller, request, custom` with `custom == {}` and `caller.host` equal to `AtmCronDeps.host`.
- [ ] (REQ-BK-D4A) `boundary:CronScheduler` with `cron_expr = "0 * * * * * *"` (every minute) and the paused clock advanced 180 minutes in one step, the runner is called once, not 180 times, and `next` is the first minute after the new now.
- [ ] (REQ-BK-D4B) `boundary:CronScheduler` a runner that takes 90 s under a 60 s schedule yields one call and one `skipped_in_flight` log entry for the second tick.
- [ ] (REQ-BK-LOG) `boundary:CronScheduler` every tick writes one `atm_cron` `tick` entry with `team`, `phase`, `hook == "cron.tick"`, `schedule`, `outcome`, `elapsed_ms`, and every `Hook` stop check one `stop_check` entry with `hook == "cron.stop"`, captured with the observability test port; an `Actions` reply's per-action entries are written by the dispatcher (bk-3, ADR-068 D7) and the `RecordingHookActionDispatcher` test confirms `dispatch_actions` received them.
- [ ] (ADR-069 D1) `boundary:CronScheduler` `start` with `cron_expr = "x y"` returns `ATM_CRON_EXPR_INVALID` whose message contains the parser's text and the store's `start` was not called; with `"0 0 2 * * * *"` the row is returned with `cron_expr` byte-identical.
- [ ] (ADR-069 D2) `boundary:CronScheduler` after `start_worker` with one row, `start` of a second schedule makes it tick at its next occurrence without restarting the worker; `end` of the first stops its ticks.
- [ ] `boundary:CronScheduler` `shutdown(deadline)` with a run in flight returns before the deadline and the worker handle is joined (`is_finished()`).
- [ ] (ANC-025) `boundary:CronScheduler` a schedule `Until { at: now + 90 s }` on an every-minute expression ticks once at 60 s and at 90 s is ended: `active == false`, `ended_at == now + 90 s`, `last_outcome == "stopped: until reached"`, the runner was called once, and the slot is gone (no call at 120 s).
- [ ] (ANC-024) `boundary:CronScheduler` a `Hook` schedule whose `DummyHookRunner.stop_replies` yields `{"stop": true, "reason": "phase bead closed"}` records `last_outcome == "stopped: phase bead closed"`, `active == false`, and `DummyHookRunner.calls` holds one `cron.stop` request and no `cron.tick` request; with `{"stop": false}` the calls are one `cron.stop` then one `cron.tick`; with `Err(HookMalformed)` the row stays active, `last_outcome == "stop_check_failed: ATM_HOOK_MALFORMED"` and no `cron.tick` ran.
- [ ] (ANC-023) `boundary:CronScheduler` `AtmCron::start` with a `NewCronSchedule` is the only way a row is written and the type carries `stop: StopCondition` (no `Option`), so none-or-both is unrepresentable here (the CLI usage error is bk-5, the CHECK refusal bk-1); the `cron.stop` request has the same seven top-level keys as `cron.tick` and `request == {"args": [...]}`.
- [ ] (REQ-BK-D3) `boundary:CronScheduler` `rg -n 'process::Command|DaemonHookRunner|HookRunner for' crates/atm-cron/src` is empty: the crate receives the router's runner and never builds one.
- [ ] `boundary:CronScheduler` `cargo tree -p atm-cron -e normal` lists no `atm-http-runtime`, `atm-daemon-bootstrap`, `atm`, `atm-storage-rusqlite`, `atm-herdr`, `atm-graft` or `tokio-cron-scheduler`; `just lint` boundary check passes with `boundaries/atm-cron/cron-scheduler.toml` and the lint-config entry.

## Authoritative validation

- `cargo test -p atm-cron`
- `cargo clippy -p atm-cron --all-targets -- -D warnings`
- `cargo build --workspace`
- `just lint`

## Non-closure

- `atm cron start|end|list` (bk-5 CLI, bk-3 routes) is the only producer of
  rows in production; this sprint's tests call `CronScheduler::start` and
  `end` directly. `atm cron end` is the manual stop and is a route (bk-3)
  over this crate's `end`.
- No file outside `crates/atm-cron` and the four edited files is touched;
  bootstrap composition (`AtmCron::new`, `start_worker`, `DaemonWorkers.cron`,
  the shutdown step, `with_cron_scheduler` on the router) is bk-5's.
- A live tick through `DaemonHookRunner` and a fixture module
  (`fixture_cron_receives_args`) is bk-5's integration criterion.
