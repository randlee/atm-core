# Sprint bk-3 — hook runner and the task-command filter

closure_type `boundary`; target_boundary `BOUNDARY-HookRunner` (daemon
implementation) and `BOUNDARY-TaskCommands` (the normal path and the hook
filter); branch `sprint/bk-3-hook-runner`, PR → the stack top above bk-2
(`sprint/bk-2-hook-contract`); difficulty hard; recommended_agent
`arch-ctm`, recommended_model `gpt-5-codex`; parallel_safe with bk-4
(disjoint owned paths), rebased onto the stack before bk-4. Consumes bk-1's store, rows, codes and budget constants and bk-2's
traits, types, protocol variants and routes, all through
`atm_core::boundary`. Governing text:
`design.md` §2.2, §3 (D-3, D-5, D-8, D-10), §9.3, §9.5, §9.6, §9.7, §9.8, §9.9,
§9.11; `REQ-P-HOOK-001` §23.2, `REQ-P-HOOK-002` §23.3; ADR-068 D1, D2,
D4, D5, D7, ADR-066, ADR-067.

## Scope boundary

This sprint owns the one process that runs hooks, the extraction of
today's task path into `NormalTaskCommands`, the filter
`HookedTaskCommands` that sits between the HTTP input and that path, the
base router's `HookActionDispatcher` implementation, the snapshot, and the
daemon side of the six registry and schedule routes. It edits nothing
under `crates/atm/src`, `crates/atm-core`, `crates/atm-storage*` or
`crates/atm-cron`.

Owned paths:

- `crates/atm-daemon-bootstrap/src/process_deadline.rs` (new),
  `crates/atm-daemon-bootstrap/src/hook_runner.rs` (new:
  `DaemonHookRunner`), `crates/atm-daemon-bootstrap/src/lib.rs` (edited:
  mods), `crates/atm-daemon-bootstrap/src/received_hook_selector.rs`
  (edited: `run_tmux` at `:458` builds a `SpawnSpec` and calls
  `spawn_with_deadline`; its tests unchanged),
  `crates/atm-daemon-bootstrap/src/replacement_handler.rs` (edited:
  `compose_storage_router` `:268` and `build_replacement_handler` `:204`
  per `design.md` §9.9)
- `crates/atm-http-runtime/src/task_commands.rs` (new:
  `NormalTaskCommands`), `crates/atm-http-runtime/src/hook_filter.rs`
  (new: `ActiveHookSnapshot`, `HookedTaskCommands`, the log entries),
  `crates/atm-http-runtime/src/hook_actions.rs` (new: `impl
  HookActionDispatcher for StorageAndNudgeRouter`),
  `crates/atm-http-runtime/src/hook_registry_routes.rs` (new: the six
  arms), `crates/atm-http-runtime/src/storage_and_nudge_router.rs`
  (edited: fields of `design.md` §9.9 on the struct at `:54`;
  `with_task_commands`, `with_hook_runner`, `with_local_host`,
  `install_cron_scheduler`; `write_unhooked` holding today's body from
  `:991`; the classify-and-call step in `write_with_request_id` (`:973`)
  after `:988-989` and in the `TaskMove` and `Messages(List)` arms of
  `dispatch_non_write` (`:532`); six registry and schedule arms),
  `crates/atm-http-runtime/src/message_handler.rs` (edited: the six
  routes next to `canonical_write_path()` at `:276`, their
  `decode_framework_request` (`:471`) arms, and `map_api_response`
  (`:707`) setting `X-ATM-Hook-Actions` for `HookActions`),
  `crates/atm-http-runtime/src/lib.rs` (edited: mods)
- `crates/atm-http-runtime/tests/hook_fixture/fixture_hook.py` (new)
- `crates/atm-architecture/tests/process_spawn_sites.rs` (new)
- `boundaries/atm-core/hook-runner.toml`, `boundaries/atm-core/task-commands.toml`,
  `boundaries/atm-core/hook-action-dispatcher.toml` (edited: `[status]`
  `concrete_landed`, `notes` naming `DaemonHookRunner`,
  `NormalTaskCommands`, `HookedTaskCommands` and `StorageAndNudgeRouter`;
  visibility stays `trait_only`),
  `boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml` (edited:
  `io_owns` + `hook_process_spawn`),
  `boundaries/atm-http-runtime/http-runtime.toml` (edited: `io_owns` +
  `hook_filter`, `hook_action_dispatch`)

## Public contract

```rust
// crates/atm-daemon-bootstrap/src/process_deadline.rs   (design.md §9.7)
pub(crate) struct SpawnSpec { pub program: PathBuf, pub args: Vec<OsString>, pub env: Vec<(OsString, OsString)>, pub cwd: PathBuf }
pub(crate) struct SpawnedOutput { pub stdout: Vec<u8>, pub stderr: Vec<u8>, pub status: std::process::ExitStatus, pub elapsed_ms: u64 }
pub(crate) const STDOUT_CAP: u64 = 1 << 20;  pub(crate) const STDERR_CAP: u64 = 64 << 10;
/// Builds the Command (env_clear, envs, current_dir, kill_on_drop, piped stdio, unix process_group(0)), writes and closes stdin,
/// joins wait + bounded take() reads under one tokio::time::timeout(remaining). Elapsed: killpg (unix) / kill (windows), then Err(hook_timeout).
/// Over a cap: kill, Err(hook_malformed). Spawn error: Err(hook_exit naming program and cwd). Never retries.
pub(crate) async fn spawn_with_deadline(spec: SpawnSpec, stdin: &[u8], remaining: Duration) -> Result<SpawnedOutput, AtmError>;

// crates/atm-daemon-bootstrap/src/hook_runner.rs
pub struct DaemonHookRunner { python: Result<PathBuf, String>, path: Option<OsString>, home: Option<OsString> }
impl DaemonHookRunner { pub fn new(env: &dyn EnvSource) -> Self; }   // copies PATH and HOME once, resolves python3 (python on Windows) on that PATH; never fails
impl atm_core::boundary::sealed::Sealed for DaemonHookRunner {}
#[async_trait] impl HookRunner for DaemonHookRunner { /* run, doctor, stop as bk-2; argv = [module_path, hook name | "doctor"]; doctor writes HookDoctorRequest::for_registration and returns HookDoctorReply::parse */ }
// env: ATM_TEAM, ATM_PHASE, PATH, HOME; Windows adds SystemRoot and USERPROFILE (design.md D-1). A missing python, PATH or HOME: Err(hook_exit naming it).

// crates/atm-http-runtime/src/task_commands.rs
pub struct NormalTaskCommands { router: StorageAndNudgeRouter }   // the base router clone, no filter installed
impl NormalTaskCommands { pub fn new(base: StorageAndNudgeRouter) -> Self; }
impl atm_core::boundary::sealed::Sealed for NormalTaskCommands {}
#[async_trait] impl TaskCommands for NormalTaskCommands {}   // assign/start/close -> write_unhooked; move_task -> task_move; list/events -> list_messages

// crates/atm-http-runtime/src/hook_filter.rs
pub struct ActiveHookSnapshot(std::sync::RwLock<HashMap<(TeamName, HookName), Vec<HookRegistration>>>);   // one row per active phase, registered_at order
impl ActiveHookSnapshot { pub fn load(store: &dyn HookRegistrationStore) -> Result<Self, AtmError>;
    pub fn refresh(&self, store: &dyn HookRegistrationStore) -> Result<(), AtmError>;            // replaces the map from active_all()
    pub fn active(&self, team: &TeamName, hook: &HookName) -> Vec<HookRegistration>; }       // clones; empty when none
pub struct HookedTaskCommands { pub inner: Arc<dyn TaskCommands>, pub actions: Arc<dyn HookActionDispatcher>,
                                pub snapshot: Arc<ActiveHookSnapshot>, pub runner: Arc<dyn HookRunner> }
impl atm_core::boundary::sealed::Sealed for HookedTaskCommands {}
#[async_trait] impl TaskCommands for HookedTaskCommands {}   // each method: filter(TaskCommand::X(req), ctx)
```

Each `HookedTaskCommands` method runs these steps in order (`design.md`
D-5, D-10):

1. `ctx.ingress != AuthenticatedIngress::Local`: call `inner` (D-f).
2. `snapshot.active(ctx.caller_team, hook)` is empty: call `inner`, with
   no spawn and no store read (a plug-in with two of six hooks passes the
   other four through here).
3. In one `tokio::spawn` over cloned `Arc`s, for each registration in
   order: build `HookRequest::for_task(&reg, caller, &command)`; its
   deadline is `min(reg.timeout_ms, remaining - HOOK_ACTION_RESERVE_MS)`
   of the one request deadline, and a non-positive value is
   `hook_timeout(reg.phase, "request budget exhausted before the hook
   ran")`; then `runner.run`. A `JoinError` with `is_panic()` maps to
   `hook_exit("hook runner panicked: <payload>")` and the daemon keeps
   serving.
4. Decide. A runner `Err` from any phase is returned at once and no action
   runs; a malformed reply never falls through to `inner`.
   `NotApplicable` moves to the next registration. Any other reply decides
   and the chain stops: later registrations are not run.
   - `Refused { message }` is `AtmError::hook_refused(message)`.
   - `PassThrough`, or all `NotApplicable`: call `inner` with the original request.
   - `Handled(list)` calls `actions.dispatch_actions(hook, team, deciding
     phase, ctx.caller_identity, list, Some(ctx.deadline))`. It returns the
     last response whose command equals the hooked command, or else
     `ResponseEnvelope::HookActions { dispatched: n }` (D-b).

```rust
// crates/atm-http-runtime/src/hook_actions.rs
impl atm_core::boundary::sealed::Sealed for StorageAndNudgeRouter {}
#[async_trait] impl HookActionDispatcher for StorageAndNudgeRouter {}
// validates all actions first (design.md §2.2: team == registration team, `as` on the roster via validate_heartbeat_member :1106,
// into_api_request Ok), else hook_malformed and nothing runs; then each action -> self.dispatch_with_request_id(api, Local, deadline, fresh id)
// on a router whose task_commands is None (the base), so no action reaches a hook. First Err: prefix "hook action {i+1}/{n} ({command}) failed after {i} committed: ".

// crates/atm-http-runtime/src/hook_registry_routes.rs: one fn per arm; Local ingress only (pattern task_move :614);
// caller_identity on the caller_team roster (validate_heartbeat_member :1106); no lead check (ADR-068 D5).
pub(crate) async fn hook_register(router: &StorageAndNudgeRouter, req: HookRegisterRequest, ingress: AuthenticatedIngress) -> Result<ApiResponse, AtmError>;
// HookName::parse_task_hook (the only call site in atm-http-runtime), timeout_ms 1..=HOOK_TIMEOUT_MAX_MS, custom_object(req.custom), then store.register + snapshot.refresh
// in one control_path_sync_bridge.run closure (D-8); refresh failure after commit: internal error "registration committed; snapshot refresh failed; restart the daemon".
pub(crate) async fn hook_disable(..) -> ..;   pub(crate) async fn hook_list(..) -> ..;            // disable + refresh likewise
pub(crate) async fn cron_start(..) -> ..;     pub(crate) async fn cron_end(..) -> ..;   pub(crate) async fn cron_list(..) -> ..;
// cron arms: router.cron.get() None -> daemon_unavailable("cron scheduler not installed"); otherwise the trait call. cron_start first checks
// timeout_ms 1..=CRON_TIMEOUT_MAX_MS (30 000, Rand 2026-10-18; never HOOK_TIMEOUT_MAX_MS) and custom_object(req.custom), else ATM_HOOK_MODULE_INVALID,
// and builds NewCronSchedule with jitter_s = 0, which AtmCron::start replaces (bk-4); the cron expression is parsed only by the scheduler.
```

Log entries (ADR-067 fields, `tracing::info!`): `subsystem =
"atm_hook"`, `action = "run" | "action"`, `outcome = "pass_through" |
"refused" | "actions" | "exit" | "timeout" | "malformed" | "action_ok" |
"action_err"`, `team`, `phase`, `hook`, `caller`, `elapsed_ms`,
`action_count`, `action_command`, `run_as`.

## Authoritative deliverables

1. `spawn_with_deadline` and `DaemonHookRunner` as above, with the
   failure table of `design.md` §9.7. `run_tmux` moves onto the helper.
   `process_spawn_sites.rs` pins the three files that call
   `tokio::process::Command::new` (`process_deadline.rs`, atm-herdr
   `transport_cli.rs`, atm `send_to.rs`).
2. `NormalTaskCommands`: today's three bodies, moved, not changed. The
   existing task tests in `atm-http-runtime` and `crates/atm/tests` pass
   unmodified and are the proof.
3. `HookedTaskCommands` and `ActiveHookSnapshot` per the steps above,
   loaded in `compose_storage_router`; a load error aborts composition.
4. Router entry: with `task_commands` `Some`, each task request is
   classified (`TaskCommand::classify`) and sent to its method with a
   `TaskCallCtx` from the request, `local_host`, ingress, deadline and
   request id; with `None` the existing body runs. History is not a task
   command (D-h); `dry_run` writes are classified.
5. The base router's `HookActionDispatcher` per above. Action dispatch and
   cron dispatch both use it.
6. The six arms and routes; `map_api_response` writes `X-ATM-Hook-Actions:
   <n>` with body `{"dispatched": n}`.
7. Composition per `design.md` §9.9: `build_replacement_handler` builds
   one `DaemonHookRunner` and returns it and the base router as
   `Arc<dyn HookActionDispatcher>` in its four-tuple; the filter holds the
   same two `Arc`s and bk-5 hands them to `AtmCron` (REQ-BK-D3, ANC-021).
8. `fixture_hook.py` uses the `design.md` §9.8 `__main__` block; its
   handlers read the reply to print from a file named `fixture_reply.json`
   in its `cwd`, write the stdin request to
   `fixture_request.json` (raw bytes) and `os.environ` keys to `fixture_env.json`, and
   implement `task.close` only unless the test writes `fixture_hooks.txt`.
9. Tests, same unit, deterministic, no live daemon (REQ-BK-TESTS): the
   runner tests are `#[tokio::test(start_paused = true)]` with a real child
   of `fixture_hook.py`; the timeout case (a fixture sleeping 30 s under
   `timeout_ms = 200`) fires on the auto-advanced paused clock. Filter tests use `DummyHookRunner`,
   `RecordingHookActionDispatcher`, `RecordingTaskCommands` and
   `InMemoryHookRegistrationStore` through the router harness
   (`router_with_timeout`, `storage_and_nudge_router.rs:2008`). Log
   assertions use a `tracing_subscriber::Layer` capture, pattern
   `PromptHandoffErrorLayer` (`:1274`).

## Authoritative acceptance criteria

Runner, `crates/atm-daemon-bootstrap/src/hook_runner.rs::tests` unless stated:

- [ ] (ANC-012) `boundary:BOUNDARY-HookRunner` `runner_timeout_kills_child_and_returns_hook_timeout`: `ATM_HOOK_TIMEOUT`, and the child pid no longer exists after the call returns; `process_deadline.rs::tests::timeout_kills_grandchild_process_group` (unix, `#[cfg(unix)]`): a fixture that forks a `sleep 30` grandchild leaves no process in its group.
- [ ] (REQ-BK-ERRORS) `boundary:BOUNDARY-HookRunner` `nonzero_exit_returns_hook_exit_with_stderr_tail` (exit 3, message holds `exit 3`), `hook_killed_by_signal_reports_signal` (unix), `module_exception_returns_hook_exit_with_traceback_tail` (`raise ValueError("boom")`, message holds `ValueError: boom`), `stdout_over_cap_is_malformed_and_child_killed`, `not_json_stdout_is_malformed`, `python3_missing_returns_hook_exit` (runner built with `PATH` set to an empty temp dir), `module_path_missing_returns_hook_exit`, `cwd_missing_returns_hook_exit`, `zero_remaining_budget_returns_timeout_without_spawn`.
- [ ] (ANC-022) `boundary:BOUNDARY-HookRunner` `custom_from_registration_reaches_stdin_byte_identical`: with `custom = {"goal":"ship","n":[1,{"y":null}]}` the `custom` bytes in `fixture_request.json` equal `serde_json::to_string(&reg.custom)`.
- [ ] (REQ-BK-D1) `boundary:BOUNDARY-HookRunner` `runner_passes_daemon_path_and_home_values`: the key set of `fixture_env.json` is exactly `{ATM_TEAM, ATM_PHASE, PATH, HOME}` on unix (plus `SystemRoot`, `USERPROFILE` on Windows), `PATH` and `HOME` equal the values given to `DaemonHookRunner::new`, and `os.getcwd()` equals the registered cwd.
- [ ] (ADR-068 D3, ADR-069 D4) `boundary:BOUNDARY-HookRunner` `doctor_sends_team_phase_custom_and_returns_findings` (argv `["doctor"]`, stdin is the §9.10 doctor request with the row's `team`, `phase`, `custom`; the §9.10 example reply returns one `HookDoctorFinding`; `{"ok":1}` is `ATM_HOOK_MALFORMED`), `stop_parses_both_shapes_and_refuses_missing_reason`: argv is `["cron.stop"]` and stdin `hook == "cron.stop"`.
- [ ] `boundary:BOUNDARY-HookRunner` `received_hook_selector.rs` tests pass unmodified; `crates/atm-architecture/tests/process_spawn_sites.rs::tokio_command_spawn_sites_are_the_three_known_files`.

Filter and normal path, `crates/atm-http-runtime/src/hook_filter.rs::tests` unless stated:

- [ ] (ANC-014) `boundary:BOUNDARY-TaskCommands` `no_registration_request_path_makes_no_store_call` (store counter unchanged after construction across write, move, list) and `unregistered_method_calls_inner_without_spawn`: registration for `task.close` only; `task.assign` reaches `inner` and `DummyHookRunner.calls.len() == 0`.
- [ ] (ANC-013) `boundary:BOUNDARY-TaskCommands` `each_method_consults_only_its_own_hook`: six cases, each registering one hook and asserting one runner call for that method and none for the other five; `task_history_never_consults_a_hook`; `dry_run_task_assign_consults_hook`; `internal_list_messages_caller_does_not_consult` (a `list_messages` call from the base router with a `task.list` hook registered).
- [ ] (ANC-001) `boundary:BOUNDARY-TaskCommands` `hook_replaces_the_call_before_any_write`: with an active `task.assign` hook, `DummyHookRunner.calls.len() == 1` before `NormalTaskCommands` is reached, and with a refusing reply `RecordingTaskCommands.calls` (as `inner`) is empty.
- [ ] (D-5) `boundary:BOUNDARY-TaskCommands` `two_phases_each_hook_handles_its_own_task` (`bj` and `bk` both register `task.close`; each close runs them oldest first until the owner handles, and dispatches its actions under its phase); `chain_not_applicable_continues_to_next_hook`; `chain_pass_through_stops_and_runs_inner` and `chain_handled_stops_and_runs_actions`; `all_not_applicable_runs_inner`; `first_handler_wins_and_later_hooks_are_not_run` (`bk` not-applicable, `bj` handled; a third phase's module is never spawned); `shared_budget_exhaustion_fails_with_hook_timeout_naming_phase` (the first reply is held on a `Notify` while the paused clock passes the deadline; `bk` is not spawned and the error holds `phase bk`).
- [ ] (ANC-004) `boundary:BOUNDARY-TaskCommands` `refused_hook_writes_nothing_and_returns_hook_refused`: `{"result":"refused","message":"bd not ready"}` gives `ATM_HOOK_REFUSED` with message `bd not ready`, and the ledger is unchanged.
- [ ] (ANC-005) `boundary:BOUNDARY-TaskCommands` `pass_through_row_equals_unhooked_row` field by field; `module_without_handler_for_hook_replies_not_applicable`: fixture registered for `task.start` but implementing only `task.close` replies `{"result":"not-applicable"}` and gives the normal result.
- [ ] (ANC-004, Rand malformed ruling) `boundary:BOUNDARY-TaskCommands` `malformed_reply_returns_error_and_never_calls_inner` (`RecordingTaskCommands.calls` empty), `empty_action_list_is_malformed`.
- [ ] (ANC-006, ANC-007, ANC-009) `boundary:BOUNDARY-HookActionDispatcher` `hook_actions.rs::tests::two_assign_actions_land_in_order_with_as_actor`: both tasks exist in list order, the original id is absent, `"as":"team-lead"` gives actor `team-lead`, no `as` gives the caller; `task_close_hook_returning_send_only_returns_hook_actions` (`dispatched == 1`, header present); `send_action_persists_a_message`.
- [ ] (D-c, D-d) `boundary:BOUNDARY-HookActionDispatcher` `second_action_failure_keeps_first_action_committed` (message carries `hook action 2/2 (task.assign) failed after 1 committed: `), `action_for_another_team_is_malformed_and_runs_nothing`, `action_as_off_roster_is_malformed_and_runs_nothing`, `hook_register_action_is_malformed`.
- [ ] (ANC-008) `boundary:BOUNDARY-HookActionDispatcher` `action_of_the_hooked_command_does_not_reenter_the_filter`: a `task.assign` hook whose reply holds a `task.assign` action ends with `DummyHookRunner.calls.len() == 1`.
- [ ] (Rand panic ruling) `boundary:BOUNDARY-TaskCommands` `runner_task_panic_returns_hook_exit_and_daemon_keeps_serving`: `DummyHookRunner.panic_on_run` set; the call returns `ATM_HOOK_EXIT` with `hook runner panicked: `, and a following un-hooked `task.list` on the same router succeeds.
- [ ] (D-f) `boundary:BOUNDARY-TaskCommands` `peer_ingress_goes_to_inner_without_consult`.
- [ ] (D-8) `boundary:BOUNDARY-TaskCommands` `register_and_disable_each_refresh_the_snapshot_once`; `concurrent_register_and_disable_leave_snapshot_equal_to_store`; `disable_during_in_flight_hook_does_not_cancel_its_actions` (the `DummyHookRunner` reply is released by a `tokio::sync::Notify` after the disable commits); `replacement_handler.rs::tests::snapshot_load_error_fails_daemon_composition`.
- [ ] (REQ-BK-LOG) `boundary:BOUNDARY-TaskCommands` `two_action_run_logs_one_run_and_two_action_entries` with `team`, `phase`, `hook`, `outcome`, `elapsed_ms` captured by the layer.
- [ ] (ANC-003, ANC-022) `boundary:BOUNDARY-TaskCommands` `runner_receives_request_body_and_the_registration_custom`: for each of the six methods the `HookRequest` in `DummyHookRunner.calls` has exactly the seven keys `schema_version, hook, team, phase, caller, request, custom`; `caller.identity`/`caller.team` equal the request's fields and `caller.host` the router's `local_host`; `request` equals the bare body the CLI sent; `custom` equals the active registration's stored `custom`.
- [ ] (ANC-007) `boundary:BOUNDARY-HookActionDispatcher` `hook_actions.rs::tests::action_body_identical_to_cli_body_gives_the_cli_result`: an action `{"command":"task.assign","request":<the body atm task assign sends>}` produces the same task row as that CLI call through the HTTP handler.
- [ ] (ANC-021, REQ-BK-D3) `boundary:BOUNDARY-HookActionDispatcher` `replacement_handler.rs::tests::filter_and_cron_share_one_runner_and_one_dispatcher`: the runner and dispatcher in the returned tuple are `Arc::ptr_eq` to those the composed `HookedTaskCommands` holds.
- [ ] (ANC-002) `boundary:BOUNDARY-TaskCommands` `hook_registry_routes.rs::tests::disable_keeps_rows_and_list_returns_them`.
- [ ] (ADR-068 D3, D-m) `boundary:BOUNDARY-TaskCommands` `hook_registry_routes.rs::tests::register_refuses_cron_tick_unknown_name_and_over_cap_timeout` (`ATM_HOOK_MODULE_INVALID` each); `register_from_non_lead_member_succeeds_and_off_roster_fails`; `registry_routes_refuse_peer_ingress`.
- [ ] (D-q, D-l) `boundary:BOUNDARY-TaskCommands` `hook_registry_routes.rs::tests::cron_start_refuses_cron_timeout_above_30s` (30 001 and 0 refused naming `CRON_TIMEOUT_MAX_MS`; 30 000 and 2 501 accepted), `::custom_non_object_refused` and `::custom_over_64k_refused` (register and cron start, `ATM_HOOK_MODULE_INVALID`, nothing stored); an absent `custom` is stored as `{}`.
- [ ] (ADR-069 D1, D4) `boundary:BOUNDARY-TaskCommands` `hook_registry_routes.rs::tests::cron_arms_pass_through_to_the_scheduler` (`RecordingCronScheduler` receives `NewCronSchedule` with `created_by == caller_identity`, `stop` and `custom` unchanged and `jitter_s == 0`; its `Err` returns unchanged) and `cron_routes_without_scheduler_are_unavailable`.
- [ ] (ANC-010) `boundary:BOUNDARY-HookRunner` `rg -n '"bd"|\.beads' crates/atm-daemon-bootstrap/src crates/atm-http-runtime/src` matches nothing.

## Authoritative validation

- `cargo test -p atm-daemon-bootstrap -p atm-http-runtime -p atm-architecture -p atm`
- `cargo clippy -p atm-daemon-bootstrap -p atm-http-runtime --all-targets -- -D warnings`
- `just lint` (boundaries, `.just/check_line_counts.py`)

## Non-closure

- No cron tick or stop check runs (bk-4). No CLI command, `TIMEOUT_MS`
  read, exit-code arm, doctor collection or `AtmCron` install (bk-5).
