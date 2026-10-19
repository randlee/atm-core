# Sprint bk-3 — hook runner and the task-command filter

closure_type `boundary`; target_boundary `BOUNDARY-HookRunner` and
`BOUNDARY-TaskCommands`; branch `sprint/bk-3-hook-runner`, PR → the stack top above bk-2;
difficulty hard; recommended_agent `arch-ctm`, recommended_model
`gpt-5-codex`; parallel_safe with bk-4 (disjoint except hunks in root
`Cargo.toml` and `.just/lint-config.toml`), rebased before bk-4. Consumes
bk-1 and bk-2 through `atm_core::boundary`.
Governing text:
`design.md` §2.2, §3 (D-3, D-5, D-8, D-10), §9.3, §9.5-§9.9, §9.11; `REQ-P-HOOK-001` §23.2, `REQ-P-HOOK-002` §23.3; ADR-068 D1, D2,
D4, D5, D7, ADR-066, ADR-067.

## Scope boundary

It edits nothing under `crates/atm/src`, `crates/atm-core`, `crates/atm-storage*` or
`crates/atm-cron`.

Owned paths:

- `crates/atm-daemon-bootstrap/src/process_deadline.rs` (new),
  `src/hook_runner.rs` (new), `src/lib.rs` (edited: mods and the
  `build_replacement_handler` call sites `:471` (binding `_hook_runner`,
  `_base_dispatcher`), `:1303`, `:1505`),
  `src/telemetry_lifecycle_tests.rs` (edited: the call at `:172`),
  `src/replacement_handler.rs` (edited: `compose_storage_router` `:268`,
  `build_replacement_handler` `:204`, `design.md` §9.9);
  `crates/atm-daemon-bootstrap/Cargo.toml` (edited: `async-trait`
  workspace dep; `[target.'cfg(unix)'.dependencies] rustix.workspace =
  true`); root `Cargo.toml` (`rustix = { version = "1.1", features
  = ["process"] }`, 1.1.4 locked)
- `.just/lint-config.toml` (edited: `async-trait`, `rustix` in the
  bootstrap allowlist `:153-155`; a `global_dependency_ownership` entry
  limiting `rustix` to `process_deadline.rs`)
- `crates/atm-http-runtime/src/` `task_commands.rs`, `hook_filter.rs`,
  `hook_actions.rs`, `hook_registry_routes.rs` (new),
  `storage_and_nudge_router.rs`
  (edited: the §9.9 fields on the struct `:54`, its builders,
  `unhooked()`, `write_unhooked` (today's body from `:991`), the
  classify-and-call step in `write_with_request_id` `:973` after
  `:988-989` and in `dispatch_non_write` `:532`, six arms),
  `message_handler.rs` (edited: six routes at
  `:276`, `decode_framework_request` `:471` and `map_api_response` `:707`
  arms),
  `lib.rs` (edited: mods)
- `crates/atm-http-runtime/tests/hook_fixture/fixture_hook.py` (new)
- `crates/atm-architecture/tests/process_spawn_sites.rs` (new)
- `boundaries/atm-core/{hook-runner,task-commands,hook-action-dispatcher}.toml` (edited: `[status]`
  `concrete_landed`, `notes` naming the implementers; `trait_only` stays),
  `boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml` (edited:
  `io_owns` + `hook_process_spawn`; `process_spawn` in `io_forbidden` with
  `[enforcement] io_forbidden_source_modules = { process_spawn =
  ["atm_daemon_bootstrap::hook_runner",
  "atm_daemon_bootstrap::replacement_handler"] }`),
  `boundaries/atm-daemon-bootstrap/daemon-observability.toml` (both
  bootstrap manifests: `async-trait`, `rustix` in `allowed_dependencies` `:25`),
  `boundaries/atm-http-runtime/http-runtime.toml` (edited: `io_owns` +
  `hook_filter`, `hook_action_dispatch`)

## Public contract

```rust
// crates/atm-daemon-bootstrap/src/process_deadline.rs   (design.md §9.7)
pub(crate) struct SpawnSpec { pub program: PathBuf, pub args: Vec<OsString>, pub env: Vec<(OsString, OsString)>, pub cwd: PathBuf }
pub(crate) struct SpawnedOutput { pub stdout: Vec<u8>, pub stderr: Vec<u8>, pub status: std::process::ExitStatus, pub elapsed_ms: u64 }
pub(crate) enum SpawnFailure { TimedOut { after_ms: u64 }, Failed(AtmError) }   // TimedOut has no phase; DaemonHookRunner converts it
pub(crate) const STDOUT_CAP: u64 = 1 << 20;  pub(crate) const STDERR_CAP: u64 = 64 << 10;
/// Builds the Command (env_clear, envs, current_dir, kill_on_drop, piped stdio, unix process_group(0)), writes and closes stdin, joins wait +
/// bounded take() reads under one timeout(remaining). Elapsed: unix rustix::process::kill_process_group(Pid::from_raw(child.id() as i32),
/// Signal::KILL), then kill().await, TimedOut. Over a cap: kill, Failed(hook_malformed). Spawn error: Failed(hook_exit(Spawn{..})). Never retries.
pub(crate) async fn spawn_with_deadline(spec: SpawnSpec, stdin: &[u8], remaining: Duration) -> Result<SpawnedOutput, SpawnFailure>;

// crates/atm-daemon-bootstrap/src/hook_runner.rs
pub struct DaemonHookRunner { base_env: Result<Vec<(OsString, OsString)>, AtmError>, python: Result<PathBuf, AtmError> }
impl DaemonHookRunner { pub fn new(env: &dyn EnvSource) -> Self; }   // built once (REQ-BK-D3); never fails; errors are hook_exit(Missing(..)) returned per call
/// Pure (design.md D-1): ATM_TEAM, ATM_PHASE, PATH, HOME (unix: PATH and HOME required); windows: HOME optional, SystemRoot and USERPROFILE
/// required, PATH looked up case-insensitively; python resolved on that PATH with PATHEXT. A missing required key is hook_exit(Missing(<key>)).
pub(crate) fn hook_env(env: &dyn EnvSource, team: &TeamName, phase: &PhaseId, windows: bool) -> Result<Vec<(OsString, OsString)>, AtmError>;
impl atm_core::boundary::sealed::Sealed for DaemonHookRunner {}
#[async_trait] impl HookRunner for DaemonHookRunner {}   // argv [module_path, request.hook] for run/stop, [module_path, "doctor"] for doctor; run and stop alone
// compute min(target.timeout_ms, remaining - HOOK_ACTION_RESERVE_MS) (<= 0: hook_timeout without spawn); doctor min(timeout_ms, remaining);
// TimedOut -> hook_timeout(target.phase, "no reply within <n> ms"); exit code n -> hook_exit(Exit{n, tail}); signal -> hook_exit(Signal(n)).

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
// D defaults to UnhookedRouter (the only production dispatcher, sealed); filter unit tests use D = RecordingHookActionDispatcher
pub struct HookedTaskCommands<D: HookActionDispatcher = UnhookedRouter> { inner: Arc<dyn TaskCommands>, actions: Arc<D>, snapshot: Arc<ActiveHookSnapshot>, runner: Arc<dyn HookRunner> }
impl<D: HookActionDispatcher> HookedTaskCommands<D> { pub fn new(inner: Arc<dyn TaskCommands>, actions: Arc<D>, snapshot: Arc<ActiveHookSnapshot>, runner: Arc<dyn HookRunner>) -> Self; }
// #[cfg(test)] accessors runner() and actions() return the held Arcs (ptr_eq tests)
impl atm_core::boundary::sealed::Sealed for HookedTaskCommands {}
#[async_trait] impl TaskCommands for HookedTaskCommands {}   // each method: filter(TaskCommand::X(req), ctx)
```

Each `HookedTaskCommands` method runs these steps in order (`design.md`
D-5, D-10):

1. `ctx.ingress != AuthenticatedIngress::Local`: call `inner` (D-f).
2. `snapshot.active(ctx.caller_team, hook)` is empty: call `inner`.
3. In one `tokio::spawn` over cloned `Arc`s, for each registration in
   order: when `ctx.deadline.remaining() <= HOOK_ACTION_RESERVE_MS` return
   `hook_timeout(reg.phase, "request budget exhausted before the hook
   ran")` without calling the runner; else build
   `HookRequest::for_task(&HookTarget::from(&reg), ..)` and call
   `runner.run(.., ctx.deadline)` with the deadline unchanged . A panicking `JoinError` (runner or action
   dispatch) maps to `hook_exit(Panic(payload))`.
4. Decide. A runner `Err` from any phase is returned unchanged;
   no action runs and `inner` is not called. `NotApplicable` moves to the
   next registration; any other reply decides and stops the chain.
   - `Refused { message }` is `AtmError::hook_refused(message)`.
   - `PassThrough`, or all `NotApplicable`: call `inner` with the original request.
   - `Handled(list)` calls `actions.dispatch_actions(hook, team, deciding
     phase, ctx.caller_identity, list, Some(ctx.deadline), dry_run)`, where
     `dry_run` is the write's flag. It returns the last response whose
     command equals the hooked command, else `HookActions { dispatched: n }`
     (D-b); a dry run returns `HookActions { dispatched: 0 }`.

```rust
// crates/atm-http-runtime/src/hook_actions.rs
pub struct UnhookedRouter(StorageAndNudgeRouter);   // private field; the only constructor:
impl StorageAndNudgeRouter { pub fn unhooked(&self) -> UnhookedRouter; }   // a clone with task_commands = None
impl atm_core::boundary::sealed::Sealed for UnhookedRouter {}  #[async_trait] impl HookActionDispatcher for UnhookedRouter {}
// validates all actions first (design.md §2.2, §9.2: into_api_request(team, default_as) Ok, `as` on the roster via validate_heartbeat_member :1106),
// else hook_malformed and nothing runs; dry_run: Ok(vec![]) here; then each action -> self.0.dispatch_with_request_id(api, Local, deadline,
// fresh id), so no action reaches a filter. First Err: prefix "hook action {i+1}/{n} ({command}) failed after {i} committed: ".

// hook_registry_routes.rs: Local ingress only (pattern task_move :614); caller on the team roster (:1106); no lead check (ADR-068 D5).
pub(crate) async fn hook_register(router: &StorageAndNudgeRouter, req: HookRegisterRequest, ingress: AuthenticatedIngress) -> Result<ApiResponse, AtmError>;
// HookName::parse_task_hook (sole call site), timeout_ms 1..=HOOK_TIMEOUT_MAX_MS (the only cap check), custom_object(req.custom), then store.register + snapshot.refresh
// in one control_path_sync_bridge.run closure (D-8); refresh failure after commit: internal error "registration committed; snapshot refresh failed; restart the daemon".
pub(crate) async fn hook_disable(..) -> ..;   pub(crate) async fn hook_list(..) -> ..;            // disable + refresh likewise
pub(crate) async fn cron_start(..) -> ..;     pub(crate) async fn cron_end(..) -> ..;   pub(crate) async fn cron_list(..) -> ..;
// cron arms: no scheduler -> daemon_unavailable("cron scheduler not installed"). cron_start checks timeout_ms 1..=CRON_TIMEOUT_MAX_MS
// (Rand 2026-10-18) and custom_object, else ATM_HOOK_MODULE_INVALID, then CronScheduler::start(CronStartSpec); only the scheduler parses cron_expr.
```

Log entries (ADR-067 fields, `tracing::info!`): `subsystem = "atm_hook"`, `action = "run" | "action"`, `outcome` one of
`not_applicable`, `pass_through`, `refused`, `actions` (handled), `exit`, `timeout`,
`malformed`, `action_ok`, `action_err`, one run entry per chain hook, `team`, `phase`, `hook`, `caller`, `elapsed_ms`,
`action_count`, `action_command`, `run_as`.

## Authoritative deliverables

1. `spawn_with_deadline` and `DaemonHookRunner` as above (failure table
   `design.md` §9.7); `hook_runner.rs` spawns only through the helper.
   `process_spawn_sites.rs`: a `syn` visitor over non-test `crates/*/src`
   finds `std`/`tokio` `Command` construction and asserts the file set
   equals a literal list the developer records (including
   `process_deadline.rs`, `received_hook_selector.rs`).
2. `NormalTaskCommands`: today's three bodies, moved, not changed; the
   existing task tests pass unmodified.
3. `HookedTaskCommands` and `ActiveHookSnapshot` per the steps above; a
   snapshot load error aborts composition.
4. Router entry: with `task_commands` `Some`, each task request is
   classified and sent to its method with a `TaskCallCtx`; with `None`
   the existing body runs. History is not a task command (D-h); `dry_run`
   writes are classified.
5. `UnhookedRouter`, the only production dispatcher (filter and cron).
6. The six arms and routes; `HookActions` is sent as `X-ATM-Hook-Actions:
   <n>` with body `{"dispatched": n}`.
7. Composition per `design.md` §9.9: `build_replacement_handler` returns
   its one `DaemonHookRunner` and `Arc<UnhookedRouter>` in its four-tuple
   for bk-5's `AtmCron`; all four call sites compile here.
8. `fixture_hook.py` uses the §9.8 `__main__` block; it prints
   `fixture_reply.json` from its `cwd`, writes stdin to
   `fixture_request.json` and `os.environ` to `fixture_env.json`, and
   implements `task.close` only unless `fixture_hooks.txt` says otherwise.
9. Tests, deterministic, no live daemon (REQ-BK-TESTS): runner tests are `#[tokio::test(start_paused = true)]` with a real
   `fixture_hook.py` child (timeout: a 30 s sleep under `timeout_ms = 200`
   on the auto-advanced clock). Filter tests use the doubles and the
   router harness (`router_with_timeout`, `storage_and_nudge_router.rs:2008`), logs a
   `tracing_subscriber::Layer` (pattern `PromptHandoffErrorLayer` `:1274`).

## Authoritative acceptance criteria

Runner, `crates/atm-daemon-bootstrap/src/hook_runner.rs::tests` unless stated:

- [ ] (ANC-012) `boundary:BOUNDARY-HookRunner` `runner_timeout_kills_child_and_returns_hook_timeout`: `ATM_HOOK_TIMEOUT`, the child pid gone on return; `process_deadline.rs::tests::timeout_kills_grandchild_process_group` (`#[cfg(unix)]`): a forked `sleep 30` grandchild leaves no process in the group.
- [ ] (REQ-BK-ERRORS) `boundary:BOUNDARY-HookRunner` each message pinned to §9.1/§9.7: `nonzero_exit_returns_hook_exit_with_stderr_tail` (`HOOK.EXIT: exit 3: <tail>`), `hook_killed_by_signal_reports_signal` (unix, `HOOK.EXIT: terminated by signal 9`), `module_exception_returns_hook_exit_with_traceback_tail` (holds `ValueError: boom`), `stdout_over_cap_is_malformed_and_child_killed`, `not_json_stdout_is_malformed`, `python3_missing_returns_hook_exit` (`PATH` an empty temp dir; `HOOK.EXIT: python3 not found on PATH`), `module_path_missing_returns_hook_exit` and `cwd_missing_returns_hook_exit` (`HOOK.EXIT: cannot run <path>: `), `zero_remaining_budget_returns_timeout_without_spawn`; the timeout message is `HOOK.TIMEOUT: phase <p>: no reply within 200 ms`.
- [ ] (ANC-022) `boundary:BOUNDARY-HookRunner` `custom_from_registration_reaches_stdin_byte_identical`: the `custom` bytes in `fixture_request.json` equal `serde_json::to_string(&reg.custom)` for `{"goal":"ship","n":[1,{"y":null}]}`.
- [ ] (REQ-BK-D1) `boundary:BOUNDARY-HookRunner` `runner_passes_daemon_path_and_home_values`: `fixture_env.json` keys are exactly `{ATM_TEAM, ATM_PHASE, PATH, HOME}` on unix, values as given to `new`, `os.getcwd()` the registered cwd; `hook_env_rules` (pure, both `windows` values): unix without `HOME` or `PATH` is `hook_exit(Missing)`; windows without `HOME` succeeds, without `SystemRoot` or `USERPROFILE` fails, and `Path` is found as `PATH`; `#[cfg(windows)] windows_runner_works_without_home_and_with_path_cased_Path`.
- [ ] (ADR-068 D3, ADR-069 D4) `boundary:BOUNDARY-HookRunner` `doctor_sends_team_phase_custom_and_returns_findings` (argv[1] `doctor` for a registration and a schedule target, stdin the §9.10 request; the §9.10 example gives one finding; `{"ok":1}` is `ATM_HOOK_MALFORMED`), `stop_parses_both_shapes_and_refuses_missing_reason` (argv[1] and stdin `hook` are `cron.stop`).
- [ ] `boundary:BOUNDARY-HookRunner` `received_hook_selector.rs` is unchanged; `process_spawn_sites.rs::process_spawn_sites_are_the_recorded_files`; `just lint` passes with `process_spawn` forbidden in `hook_runner` and `replacement_handler`.

Filter and normal path, `crates/atm-http-runtime/src/hook_filter.rs::tests` unless stated:

- [ ] (ANC-014) `boundary:BOUNDARY-TaskCommands` `no_registration_request_path_makes_no_store_call` (store counter unchanged across write, move, list) and `unregistered_method_calls_inner_without_spawn` (`task.close` registered; `task.assign` reaches `inner`, zero runner calls).
- [ ] (ANC-013) `boundary:BOUNDARY-TaskCommands` `each_method_consults_only_its_own_hook`: six cases, one runner call for the registered method and none for the other five; `task_history_never_consults_a_hook`; `dry_run_task_assign_consults_hook`; `internal_list_messages_caller_does_not_consult` (base-router `list_messages` with a `task.list` hook).
- [ ] (ANC-001) `boundary:BOUNDARY-TaskCommands` `hook_replaces_the_call_before_any_write`: one runner call before `NormalTaskCommands` is reached; with a refusing reply `RecordingTaskCommands.calls` is empty.
- [ ] (D-5) `boundary:BOUNDARY-TaskCommands` `two_phases_each_hook_handles_its_own_task` (`bj`, `bk` on `task.close`; oldest first until the owner handles, actions under its phase); `chain_not_applicable_continues_to_next_hook`; `chain_pass_through_stops_and_runs_inner`; `chain_handled_stops_and_runs_actions`; `all_not_applicable_runs_inner`; `chain_not_applicable_then_refused_returns_refused_and_skips_inner`; `chain_not_applicable_then_error_fails_command_and_never_runs_inner`; `chain_not_applicable_then_pass_through_runs_inner_once`; `filter_passes_ctx_deadline_unchanged_to_runner`; `first_handler_wins_and_later_hooks_are_not_run` (a third phase is never spawned); `shared_budget_exhaustion_fails_with_hook_timeout_naming_phase` (first reply held on a `Notify` past the deadline; `bk` not spawned, error holds `phase bk`).
- [ ] (ANC-004) `boundary:BOUNDARY-TaskCommands` `refused_hook_writes_nothing_and_returns_hook_refused`: `{"result":"refused","message":"bd not ready"}` gives `ATM_HOOK_REFUSED` `bd not ready`, ledger unchanged.
- [ ] (ANC-005) `boundary:BOUNDARY-TaskCommands` `pass_through_row_equals_unhooked_row` field by field; `module_without_handler_for_hook_replies_not_applicable`: registered for `task.start`, implementing only `task.close`, gives the normal result.
- [ ] (ANC-004, Rand malformed ruling) `boundary:BOUNDARY-TaskCommands` `malformed_reply_returns_error_and_never_calls_inner` (`RecordingTaskCommands.calls` empty), `empty_action_list_is_malformed`.
- [ ] (ANC-006, ANC-007, ANC-009) `boundary:BOUNDARY-HookActionDispatcher` `hook_actions.rs::tests::two_assign_actions_land_in_order_with_as_actor`: both tasks in order, the original id absent, `"as":"team-lead"` the actor, else the caller; `task_close_hook_returning_send_only_returns_hook_actions` (`dispatched == 1`, header present); `handled_returns_last_action_response_for_hooked_command`; `send_action_persists_a_message`; `dry_run_handled_reply_dispatches_nothing` (`dispatched == 0`, ledger unchanged, a malformed action still fails).
- [ ] (D-c, D-d) `boundary:BOUNDARY-HookActionDispatcher` `second_action_failure_keeps_first_action_committed` (message carries `hook action 2/2 (task.assign) failed after 1 committed: `), `action_for_another_team_is_malformed_and_runs_nothing`, `action_as_off_roster_is_malformed_and_runs_nothing`, `hook_register_action_is_malformed`; `hook_filter.rs::tests::handled_second_action_fails_returns_prefixed_error_and_inner_not_called`.
- [ ] (ANC-008) `boundary:BOUNDARY-HookActionDispatcher` `action_of_the_hooked_command_does_not_reenter_the_filter`: a `task.assign` action from a `task.assign` hook leaves one runner call.
- [ ] (Rand panic ruling) `boundary:BOUNDARY-TaskCommands` `runner_task_panic_returns_hook_exit_and_daemon_keeps_serving`: `panic_on_run` gives `HOOK.EXIT: hook runner panicked: ` and a following `task.list` succeeds; `panic_in_action_dispatch_returns_hook_exit` (`RecordingHookActionDispatcher.panic_at = Some(0)`).
- [ ] (D-f) `boundary:BOUNDARY-TaskCommands` `peer_ingress_goes_to_inner_without_consult`.
- [ ] (D-8) `boundary:BOUNDARY-TaskCommands` `register_and_disable_each_refresh_the_snapshot_once`; `concurrent_register_and_disable_leave_snapshot_equal_to_store`; `concurrent_register_same_key_leaves_one_row_and_snapshot_equals_store`; `snapshot_order_equals_store_order`; `disable_during_in_flight_hook_does_not_cancel_its_actions` (reply released by a `Notify` after the disable commits); `replacement_handler.rs::tests::snapshot_load_error_fails_daemon_composition`.
- [ ] (REQ-BK-LOG) `boundary:BOUNDARY-TaskCommands` `two_action_run_logs_one_run_and_two_action_entries`, each with the five ADR-067 fields; `two_phase_chain_logs_not_applicable_then_actions` (`bk` `not_applicable`, `bj` `actions`).
- [ ] (ANC-003, ANC-022) `boundary:BOUNDARY-TaskCommands` `runner_receives_request_body_and_the_registration_custom`: for each method the `HookRequest` has the seven keys; `caller` is the request's identity and team and the router's `local_host`; `request` is the CLI's bare body; `custom` the registration's.
- [ ] (ANC-007) `boundary:BOUNDARY-HookActionDispatcher` `hook_actions.rs::tests::action_body_identical_to_cli_body_gives_the_cli_result`: a `task.assign` action carrying the CLI's body gives the CLI's task row.
- [ ] (ANC-021, REQ-BK-D3) `boundary:BOUNDARY-HookActionDispatcher` `replacement_handler.rs::tests::filter_and_cron_share_one_runner_and_one_dispatcher`: the tuple's runner and `UnhookedRouter` are `Arc::ptr_eq` to the filter's `runner()` and `actions()`; one `DaemonHookRunner` per composition; task hooks, cron and doctor parse with `HookReply::parse`.
- [ ] (ANC-002, ADR-068 D3, D-m) `boundary:BOUNDARY-TaskCommands` `hook_registry_routes.rs::tests::disable_keeps_rows_and_list_returns_them`; `register_refuses_cron_tick_unknown_name_and_over_cap_timeout` (`ATM_HOOK_MODULE_INVALID` each); `register_from_non_lead_member_succeeds_and_off_roster_fails` and `off_roster_caller_refused_on_every_registry_route` (disable, list and the three cron arms); `registry_routes_refuse_peer_ingress` (all six routes); `reregister_active_key_keeps_chain_position_in_snapshot`.
- [ ] (D-q, D-l) `boundary:BOUNDARY-TaskCommands` `hook_registry_routes.rs::tests::cron_start_refuses_cron_timeout_above_30s` (30 001 and 0 refused naming `CRON_TIMEOUT_MAX_MS`; 30 000, 2 501 accepted), `::custom_non_object_refused`, `::custom_over_64k_refused` (both routes, nothing stored); absent `custom` is `{}`.
- [ ] (ADR-069 D1, D4) `boundary:BOUNDARY-TaskCommands` `hook_registry_routes.rs::tests::cron_arms_pass_through_to_the_scheduler` (`RecordingCronScheduler` receives a `CronStartSpec` with `created_by == caller_identity`, `stop` and `custom` unchanged; its `Err` returns unchanged) and `cron_routes_without_scheduler_are_unavailable`.
- [ ] (ANC-010) `boundary:BOUNDARY-HookRunner` `rg -n '"bd"|\.beads' crates/atm-daemon-bootstrap/src crates/atm-http-runtime/src` matches nothing.

## Authoritative validation

- `cargo test -p atm-daemon-bootstrap -p atm-http-runtime -p atm-architecture -p atm`
- `cargo clippy -p atm-daemon-bootstrap -p atm-http-runtime --all-targets -- -D warnings`
- `just lint` (boundaries, `.just/check_line_counts.py`)

## Non-closure

- No cron tick or stop check runs (bk-4). No CLI command, `TIMEOUT_MS`
  read, exit-code arm, doctor collection or `AtmCron` install (bk-5).
