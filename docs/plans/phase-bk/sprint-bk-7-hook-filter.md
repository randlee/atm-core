# Sprint bk-7 — task-command filter, dispatcher and hook routes

closure_type `boundary`; target_boundary `BOUNDARY-TaskCommands` and
`BOUNDARY-HookActionDispatcher` (`atm-http-runtime`) with the hook and cron
envelope variants and routes (`atm-core` protocol, HTTP API 1.13.0); branch
`sprint/bk-7-hook-filter`, PR → `sprint/bk-3-hook-runner` (layer 5). Wave 2,
parallel with bk-6 and bk-3: cut from the bk-2 head and rebased onto layer 4
in order; it consumes from bk-2 `TaskCommands`, `TaskCommand::classify`,
`HookActionDispatcher`, `DispatchSpec`, `HookRunner` (through
`DummyHookRunner` only), `CronScheduler`, the six request structs,
`HookActionsResponse`, `check_limits` and the doctor types, and from bk-1 the
stores and `HookName::parse_task_hook`. Vertical rationale: the envelope
variants and their only exhaustive matches (`map_api_response`,
`dispatch_non_write`) change in one layer, so every layer builds. Governing
text: `design.md` §2.2, §3 (D-5, D-8, D-10, D-b, D-c, D-f, D-h), §9.2,
§9.5, §9.9-§9.11; `REQ-P-HOOK-001`, `REQ-P-HOOK-003`, `REQ-P-HOOK-004`,
`REQ-P-CRON-001`, `REQ-CORE-HOOK-001`; ADR-068 D1, D2, D5, ADR-069 D1, D4,
ADR-061 D3, D5, ADR-067.

## Scope boundary

No process spawn (bk-3), no composition (bk-5). Owned paths (new unless
marked edited; production lines measured with `.just/check_line_counts.py`
rules):

- `crates/atm-core/src/protocol.rs` (edited: six `RequestEnvelope`, seven
  `ResponseEnvelope` variants, `HTTP_API_VERSION = "1.13.0"`; 413 lines),
  `crates/atm-core/src/api.rs` (edited: `ApiRequest`/`ApiResponse` variants,
  six route constants, `HttpRouteKind` and `HTTP_ROUTE_SPECS` 16-21,
  `HOOK_ACTIONS_HEADER`, the arms of §9.11; 580 → about 670),
  `crates/atm-core/src/transport/testing.rs` (edited: arms; 172),
  `crates/atm-core/src/doctor/report.rs` (edited: `DoctorReport.hooks`; 426)
- `docs/atm-http-runtime/openapi.yaml`, `crates/atm/openapi.yaml`,
  `crates/atm/tests/openapi_surface_baseline.json`,
  `docs/adr/ADR-061-governed-interface-schema-versioning.md` (edited: the D5
  line)
- `crates/atm-http-runtime/src/task_commands.rs`,
  `crates/atm-http-runtime/src/hook_filter.rs`,
  `crates/atm-http-runtime/src/hook_actions.rs`,
  `crates/atm-http-runtime/src/hook_chains.rs`,
  `crates/atm-http-runtime/src/hook_registry_routes.rs`,
  `crates/atm-http-runtime/src/doctor_hooks.rs`,
  `crates/atm-http-runtime/src/lib.rs` (edited: mods; 772)
- `crates/atm-http-runtime/src/storage_and_nudge_router.rs` (edited; 990
  production lines): `dispatch_non_write` (`:532-585`, 54 code lines) moves
  unchanged to
  `crates/atm-http-runtime/src/storage_and_nudge_router/non_write_dispatch.rs`,
  which gains the six arms; one field `hooks: HookWiring` and its builders
  go to `crates/atm-http-runtime/src/storage_and_nudge_router/hook_wiring.rs`;
  the file keeps the
  classify-and-call in `write_with_request_id` (`:973`) and one
  `doctor_hooks::collect` call in `doctor()` (`:651`): about 944
- `crates/atm-http-runtime/src/message_handler.rs` (edited: six routes,
  `decode_framework_request` `:471` and `map_api_response` `:707` arms;
  708 → about 728)
- `boundaries/atm-core/task-commands.toml`,
  `boundaries/atm-core/hook-action-dispatcher.toml` (edited: `[status]`
  `concrete_landed` naming `NormalTaskCommands`, `HookedTaskCommands`,
  `UnhookedRouter`), `boundaries/atm-http-runtime/http-runtime.toml`
  (edited: `io_owns` + `hook_filter`, `hook_action_dispatch`)

## Public contract

```rust
// crates/atm-http-runtime/src/storage_and_nudge_router/hook_wiring.rs   (design.md §9.9)
pub(crate) struct HookWiring {
    pub task_commands: Option<Arc<dyn TaskCommands>>,         // None: the un-hooked body
    pub snapshot: Option<Arc<ActiveHookSnapshot>>,             // set by with_hook_filter; None makes registry routes unavailable
    pub runner: Option<Arc<dyn HookRunner>>,                   // doctor only
    pub cron: Arc<OnceLock<Arc<dyn CronScheduler>>>,           // shared by clones; unset: daemon_unavailable("cron scheduler is unavailable because daemon startup did not install it")
    pub chains: Arc<ChainTracker>,
    pub local_host: Option<HostName>,                           // stamped into filtered requests' caller.host
}
impl StorageAndNudgeRouter {
    pub fn with_task_commands(self, c: Arc<dyn TaskCommands>) -> Self;
    pub fn with_hook_filter<D: HookActionDispatcher + 'static>(self, filter: Arc<HookedTaskCommands<D>>) -> Self;
    pub fn with_local_host(self, host: HostName) -> Self;       pub fn hook_chains(&self) -> Arc<ChainTracker>;
    pub fn install_cron_scheduler(&self, s: Arc<dyn CronScheduler>) -> Result<(), AtmError>;   // a second call errors
    pub fn unhooked(&self) -> UnhookedRouter;                           // a clone with task_commands = None
}

// crates/atm-http-runtime/src/task_commands.rs
/// The single owner of the un-hooked bodies is the router's existing write_with_request_id body (renamed write_unhooked), task_move (:608)
/// and list_messages (:93). NormalTaskCommands, UnhookedRouter and the task_commands = None path all call those three; none copies a body.
pub struct NormalTaskCommands { router: StorageAndNudgeRouter }   // the base clone, task_commands None
impl NormalTaskCommands { pub fn new(base: StorageAndNudgeRouter) -> Self; }
impl atm_core::boundary::sealed::Sealed for NormalTaskCommands {}  #[async_trait] impl TaskCommands for NormalTaskCommands {}

// crates/atm-http-runtime/src/hook_filter.rs
type Chains = HashMap<(TeamName, HookName), Arc<[Arc<HookRegistration>]>>;   // one row per active phase, registered_at order
/// Writers replace the whole map, so a poisoned lock is recovered with into_inner (no partial state); the read lock is held only to
/// clone the Arc, never across an await. `dirty` is set when a refresh after a commit fails; the next `active` reloads first.
pub struct ActiveHookSnapshot { map: std::sync::RwLock<Arc<Chains>>, dirty: AtomicBool, store: Arc<dyn HookRegistrationStore> }
impl ActiveHookSnapshot {
    pub fn load(store: Arc<dyn HookRegistrationStore>) -> Result<Self, AtmError>;
    pub fn refresh(&self) -> Result<(), AtmError>;                                   // replaces the map from active_all(); Err sets dirty
    pub fn active(&self, team: &TeamName, hook: &HookName) -> Result<Arc<[Arc<HookRegistration>]>, AtmError>;  // reload when dirty
}
pub struct HookedTaskCommands<D: HookActionDispatcher = UnhookedRouter> {
    inner: Arc<dyn TaskCommands>, actions: Arc<D>, snapshot: Arc<ActiveHookSnapshot>, runner: Arc<dyn HookRunner>, chains: Arc<ChainTracker> }
impl<D: HookActionDispatcher> HookedTaskCommands<D> {
    pub fn new(inner: Arc<dyn TaskCommands>, actions: Arc<D>, snapshot: Arc<ActiveHookSnapshot>, runner: Arc<dyn HookRunner>, chains: Arc<ChainTracker>) -> Self;
    pub fn runner(&self) -> &Arc<dyn HookRunner>;  pub fn actions(&self) -> &Arc<D>;   // bk-5's ptr_eq composition test
}
impl<D: HookActionDispatcher + 'static> atm_core::boundary::sealed::Sealed for HookedTaskCommands<D> {}
#[async_trait] impl<D: HookActionDispatcher + 'static> TaskCommands for HookedTaskCommands<D> {}   // each method: self.filter(TaskCommand::X(req), ctx)
enum Decision { Inner, Refused(String), Done(ApiResponse) }

// crates/atm-http-runtime/src/hook_chains.rs
/// Every hook chain runs under the tracker; shutdown refuses new chains, waits for live ones up to a deadline, then aborts them.
pub struct ChainTracker { live: tokio::sync::watch::Sender<usize>, closing: AtomicBool, aborts: Mutex<HashMap<u64, AbortHandle>>, next: AtomicU64 }
impl ChainTracker {
    pub fn spawn<F: Future<Output = T> + Send + 'static, T: Send + 'static>(&self, f: F) -> Result<JoinHandle<T>, AtmError>;
        // closing: daemon_unavailable("daemon shutting down; hook chain not started")
    pub fn begin_shutdown(&self);
    pub async fn drain(&self, deadline: RequestDeadline) -> usize;     // waits for live == 0 or the deadline, aborts the rest, returns how many
}

// crates/atm-http-runtime/src/hook_actions.rs
pub struct UnhookedRouter(StorageAndNudgeRouter);                     // private field; built only by StorageAndNudgeRouter::unhooked
impl atm_core::boundary::sealed::Sealed for UnhookedRouter {}  #[async_trait] impl HookActionDispatcher for UnhookedRouter {}

// crates/atm-http-runtime/src/doctor_hooks.rs   (design.md §9.10)
pub(crate) async fn collect(router: &StorageAndNudgeRouter, team: Option<&TeamName>, deadline: RequestDeadline) -> Vec<HookDoctorEntry>;
```

The filter (`design.md` D-5, D-10, D-f); registrations are `Arc`, and each
`HookRequest` is built only when its hook runs:

```rust
async fn filter(&self, cmd: TaskCommand, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError> {
    if ctx.ingress != AuthenticatedIngress::Local { return cmd.call(&*self.inner, ctx).await; }        // D-f
    let hook = cmd.hook_name();
    let regs = self.snapshot.active(&ctx.caller_team, &hook)?;
    if regs.is_empty() { return cmd.call(&*self.inner, ctx).await; }                                // no spawn
    let caller = HookCaller { identity: ctx.caller_identity.clone(), team: ctx.caller_team.clone(), host: ctx.host.clone() };
    let (runner, actions, deadline, dry_run, body) = (self.runner.clone(), self.actions.clone(), ctx.deadline, dry_run_of(&cmd), cmd.clone_for_hook());
    let (team, who, hook2) = (ctx.caller_team.clone(), ctx.caller_identity.clone(), hook.clone());
    let chain = self.chains.spawn(async move {
        for reg in regs.iter() {
            let target = HookTarget::from(&**reg);
            let request = HookRequest::for_task(&target, hook2.clone(), caller.clone(), &body);
            match runner.run(&target, &request, deadline).await?.reply {                            // a runner Err returns unchanged
                HookReply::NotApplicable => continue,
                HookReply::PassThrough => return Ok(Decision::Inner),
                HookReply::Refused { message } => return Ok(Decision::Refused(message)),
                HookReply::Handled(list) => {
                    let n = list.len();
                    let spec = DispatchSpec { hook: hook2.clone(), team: team.clone(), phase: reg.phase.clone(), default_as: who.clone(), deadline: Some(deadline), dry_run };
                    let done = actions.dispatch_actions(spec, list).await?;                          // D-c prefix
                    let last = done.into_iter().rev().find(|(c, _)| c == hook2.as_str()).map(|(_, r)| r);  // D-b
                    return Ok(Decision::Done(if dry_run { hook_actions(0) } else { last.unwrap_or(hook_actions(n)) }));
                }
            }
        }
        Ok(Decision::Inner)                                                                         // all NotApplicable
    })?;
    match chain.await {
        Err(join) if join.is_panic() => Err(AtmError::hook_exit(HookExitCause::Panic(panic_text(join.into_panic())))),
        Err(_) => Err(AtmError::daemon_unavailable("hook chain was cancelled while the daemon was shutting down")),
        Ok(Err(error)) => Err(error),
        Ok(Ok(Decision::Refused(message))) => Err(AtmError::hook_refused(&message)),
        Ok(Ok(Decision::Done(response))) => Ok(response),
        Ok(Ok(Decision::Inner)) => cmd.call(&*self.inner, ctx).await,                                 // never inside the spawn
    }
}
// dry_run_of: the write's dry_run, false for move, list, events. hook_actions(n) = ApiResponse::HookActions(HookActionsResponse { dispatched: n }).
```

`UnhookedRouter::dispatch_actions` (design §2.2, §9.2): check every action
first (`caller_team == spec.team`; `as` on the roster via
`validate_heartbeat_member` `:1106`; `into_api_request(team, default_as)`
Ok), else `hook_malformed` and nothing runs; `dry_run` returns `Ok(vec![])`;
then each action → `self.0.dispatch_with_request_id(api, Local, deadline,
fresh id)` on the clone with no `task_commands`, so no action reaches a
filter. First `Err` gets the prefix `hook action {i+1}/{n} ({command})
failed after {i} committed: `.

Registry routes (`hook_registry_routes.rs`; Local ingress only, pattern
`task_move` `:614`; caller on the team roster `:1106`; no lead check,
ADR-068 D5): `hook_register` parses `hook` with `HookName::parse_task_hook`
(its only call site), checks `timeout_ms` in `1..=HOOK_TIMEOUT_MAX_MS` and
`check_limits` (`custom` absent is `{}`), then `store.register` and
`snapshot.refresh` in one `control_path_sync_bridge.run` closure (D-8). A
refresh failure after the commit sets `dirty` and returns
`daemon_unavailable_with_cause("registration committed; snapshot refresh
failed; the next hooked call reloads it", cause)`. `hook_disable` likewise;
`hook_list` reads the store. `cron_start` checks `timeout_ms` in
`1..=CRON_TIMEOUT_MAX_MS` (Rand 2026-10-18) and `check_limits`, else
`ATM_HOOK_MODULE_INVALID`, then `CronScheduler::start(CronStartSpec {
created_by: caller_identity, .. })`; only the scheduler parses `cron_expr`.
`cron_end`, `cron_list` pass through; scheduler errors return unchanged.

Routes (`design.md` §9.11), all `POST`: `/v1/atm/hooks/register`,
`/v1/atm/hooks/disable`, `/v1/atm/hooks/list`, `/v1/atm/cron/start`,
`/v1/atm/cron/end`, `/v1/atm/cron/list`. Responses: `HookRegistered`,
`HookList`, `CronStarted`, `CronList`, `HookActions`, and `HookDisabled`,
`CronEnded` holding the affected rows. `HookActions` is sent with header
`X-ATM-Hook-Actions: <n>` and body `{"dispatched": n}`;
`decode_http_response` checks it before `decode_success_response`, as for
`CLEAR_OUTCOME_HEADER`. Peer ingress is refused on all six.

Doctor (`design.md` §9.10): `collect` groups the team's active
registrations and schedules by `(module_path, cwd, team, phase)`; each
group is one entry naming its `hooks` and `schedules` and one
`HookRunner::doctor(target, deadline.minus(250 ms))` call with the first
row's `custom` and `timeout_ms` (registrations by hook name, then schedules
by name), run on a `JoinSet`; a failed call sets the entry's `error`. The
section never fails doctor or changes `summary`; `DoctorReport.hooks` is
`#[serde(default)] Vec<HookDoctorEntry>` without `skip_serializing_if`.

Log entries (ADR-067, `tracing::info!`): `subsystem = "atm_hook"`, `action`
`run` or `action`, `outcome` one of `not_applicable`, `pass_through`,
`refused`, `actions`, `exit`, `timeout`, `malformed`, `action_ok`,
`action_err`; one run entry per chain hook with `team`, `phase`, `hook`,
`caller`, `elapsed_ms`, `action_count`, `action_command`, `run_as`.

## Authoritative deliverables

1. Protocol: the variants, routes, header, both `openapi.yaml` copies
   (paths, the six request schemas, the `HookActions` schema and the
   `X-ATM-Hook-Actions` header), the surface baseline, `1.13.0`, and the
   ADR-061 D5 line: "1.12.0 → 1.13.0 (phase bk): six routes, seven
   responses, `DoctorReport.hooks`, six error codes; a 1.12 consumer
   ignores `hooks`, and its `AtmErrorCode::from_str` returns
   `UnknownAtmErrorCode` for the six codes".
2. `NormalTaskCommands` over the single un-hooked owner; the existing task
   tests pass unmodified.
3. `ActiveHookSnapshot`, `HookedTaskCommands`, `ChainTracker` as above.
4. Router entry: `task_commands` `Some` sends each classified request
   (`dry_run` included, history not, D-h) to its method with a
   `TaskCallCtx`; `None` runs the un-hooked owner.
5. `UnhookedRouter`, the only production dispatcher (filter and cron).
6. The six routes and arms; `doctor_hooks::collect` wired into `doctor()`.
7. Tests, deterministic, no daemon (REQ-BK-TESTS): doubles only
   (`DummyHookRunner`, `RecordingHookActionDispatcher`,
   `RecordingTaskCommands`, the in-memory stores through
   `with_hook_registration_store`), the router harness of
   `storage_and_nudge_router.rs` tests, a reply held by `before_reply` on a
   tokio `watch`, `start_paused` for drain deadlines, logs through a
   `tracing_subscriber::Layer`.

## Authoritative acceptance criteria

`crates/atm-http-runtime/src/hook_filter.rs::tests` unless stated:

- [ ] (ADR-061 D3) `boundary:BOUNDARY-AtmProtocol` `crates/atm-core/src/protocol.rs::tests::codec_preserves_protocol_1_12_0_fixtures` (pattern `codec_preserves_protocol_1_5_0_fixtures`, a pinned 1.12 enum): every 1.12 request and response round-trips, and a 1.13 `DoctorReport` with `hooks` decodes as the 1.12 struct; `api.rs::tests::hook_routes_round_trip_and_check_the_header` (each route through `encode_request_body`/`decode_framework_request`, each response through `map_api_response`/`decode_http_response`, `HookActions` without the header refused); `openapi_surface` passes with the header and schema documented.
- [ ] (D-f) `boundary:BOUNDARY-AtmProtocol` `message_handler.rs::tests::peer_ingress_hook_and_cron_routes_are_refused` (all six); `::error_response_maps_hook_codes_to_400_and_503` (`ATM_HOOK_MODULE_INVALID`, `ATM_CRON_EXPR_INVALID`, `ATM_HOOK_REFUSED` → 400; `ATM_HOOK_EXIT`, `ATM_HOOK_TIMEOUT`, `ATM_HOOK_MALFORMED` → 503; never 500).
- [ ] (ANC-014) `boundary:BOUNDARY-TaskCommands` `no_registration_request_path_makes_no_store_call` (store `calls` unchanged across write, move, list) and `unregistered_method_calls_inner_without_spawn`; `task_commands.rs::tests::existing_task_tests_pass_through_normal_task_commands` (the router harness with `NormalTaskCommands` installed runs the existing task suite unmodified).
- [ ] (ANC-013) `boundary:BOUNDARY-TaskCommands` `each_method_consults_only_its_own_hook`; `task_history_never_consults_a_hook`; `dry_run_task_assign_consults_hook`; `internal_list_messages_caller_does_not_consult`.
- [ ] (ANC-001) `boundary:BOUNDARY-TaskCommands` `hook_replaces_the_call_before_any_write`: with a refusing reply `RecordingTaskCommands.calls` is empty.
- [ ] (D-5) `boundary:BOUNDARY-TaskCommands` `two_phases_each_hook_handles_its_own_task` (`bj`, `bk`, oldest first; `DispatchCall.spec.phase == bk`); `chain_not_applicable_continues_to_next_hook`; `chain_{pass_through_stops_and_runs_inner,handled_stops_and_runs_actions}`; `all_not_applicable_runs_inner`; `chain_not_applicable_then_refused_skips_inner`; `first_handler_wins_and_later_hooks_are_not_run`; `runner_error_returns_unchanged_and_stops_the_chain` (`calls.len() == 2`, each deadline `== ctx.deadline`); `requests_are_built_only_for_hooks_that_run` (the third phase's target never appears in `calls`).
- [ ] (ANC-004, ANC-005) `boundary:BOUNDARY-TaskCommands` `refused_hook_writes_nothing_and_returns_hook_refused` (`bd not ready`, ledger unchanged); `pass_through_row_equals_unhooked_row`; `malformed_reply_error_never_calls_inner`.
- [ ] (ANC-006, ANC-007, ANC-009) `boundary:BOUNDARY-HookActionDispatcher` `hook_actions.rs::tests::two_assign_actions_land_in_order_with_as_actor`; `task_close_hook_returning_send_only_returns_hook_actions` (`dispatched == 1`, header present); `handled_returns_last_action_response_for_hooked_command`; `send_action_persists_a_message`; `dry_run_handled_reply_dispatches_nothing`; `action_body_identical_to_cli_body_gives_the_cli_result`; `hook_filter.rs::tests::handled_passes_the_dispatch_spec` (`deadline == Some(ctx.deadline)`, the write's `dry_run`, the handling phase).
- [ ] (D-c, D-d) `boundary:BOUNDARY-HookActionDispatcher` `second_action_failure_keeps_first_action_committed` (`hook action 2/2 (task.assign) failed after 1 committed: `), `action_{for_another_team,as_off_roster}_is_malformed_and_runs_nothing`, `hook_register_action_is_malformed`.
- [ ] (ANC-008) `boundary:BOUNDARY-HookActionDispatcher` `action_of_the_hooked_command_does_not_reenter_the_filter`.
- [ ] (Rand panic ruling) `boundary:BOUNDARY-TaskCommands` `runner_task_panic_returns_hook_exit_and_daemon_keeps_serving` (`panic_on_run`: `HOOK.EXIT: hook runner panicked: `, then `task.list` succeeds); `panic_in_action_dispatch_returns_hook_exit` (`panic_at = Some(0)`).
- [ ] (shutdown) `boundary:BOUNDARY-TaskCommands` `hook_chains.rs::tests::drain_waits_for_a_held_chain_then_returns_zero` (reply held by `before_reply`, released, `drain(after 10 s) == 0`, the actions committed); `::drain_aborts_at_the_deadline` (`start_paused`, never released: `drain == 1`, the caller gets `hook chain was cancelled while the daemon was shutting down`); `::new_chain_after_begin_shutdown_is_unavailable`.
- [ ] (D-8) `boundary:BOUNDARY-TaskCommands` `register_and_disable_each_refresh_the_snapshot_once`; `concurrent_register_and_disable_leave_snapshot_equal_to_store`; `snapshot_order_equals_store_order`; `disable_during_in_flight_hook_does_not_cancel_its_actions`; `refresh_failure_marks_dirty_and_the_next_call_reloads` (`fail_active_all` fails every call while set: the route returns `registration committed; snapshot refresh failed`, the next hooked call sees the row).
- [ ] (REQ-BK-LOG) `boundary:BOUNDARY-TaskCommands` `two_action_run_logs_one_run_and_two_action_entries`; `two_phase_chain_logs_not_applicable_then_actions`.
- [ ] (ANC-003, ANC-022) `boundary:BOUNDARY-TaskCommands` `runner_receives_request_body_and_the_registration_custom` (seven keys; `caller` with the router's `local_host`; `request` the CLI's bare body; `custom` the registration's).
- [ ] (ANC-002, ADR-068 D3, D5) `boundary:BOUNDARY-TaskCommands` `hook_registry_routes.rs::tests::disable_keeps_rows_and_list_returns_them`; `register_refuses_cron_tick_unknown_name_and_over_cap_timeout`; `register_same_hook_in_a_second_phase_adds_a_chain_row`; `register_from_non_lead_member_succeeds_and_off_roster_fails`; `reregister_active_key_keeps_chain_position`.
- [ ] (D-q, D-l) `boundary:BOUNDARY-TaskCommands` `cron_start_refuses_cron_timeout_above_30s` (30 001 and 0 refused naming `CRON_TIMEOUT_MAX_MS`; 30 000 accepted); `custom_non_object_and_over_64k_refused` and `oversize_name_args_and_paths_refused` (both routes, nothing stored); absent `custom` is `{}`.
- [ ] (ADR-069 D1, D4) `boundary:BOUNDARY-TaskCommands` `cron_arms_pass_through_to_the_scheduler` (`RecordingCronScheduler` gets `created_by == caller_identity`, `stop` and `custom` unchanged; its `Err` unchanged); `cron_routes_without_scheduler_are_unavailable`.
- [ ] (ANC-015, REQ-P-HOOK-004) `boundary:BOUNDARY-DaemonRuntimeDoctorReport` `doctor_hooks.rs::tests::one_entry_per_module_phase_with_first_row_custom` (two hooks and one schedule of one module and phase: one entry, the recorded `HookTarget` has that team, phase and the first registration's `custom`); `::failed_call_sets_error_and_summary_is_unchanged`; `::no_registration_gives_empty_hooks`.
- [ ] (ANC-010) `boundary:BOUNDARY-TaskCommands` `rg -n '"bd"|\.beads' crates/atm-http-runtime/src` matches nothing.

## Authoritative validation

- `cargo build --workspace`
- `cargo test -p atm-core --features test-utils -p atm-http-runtime -p atm --test openapi_surface`
- `cargo clippy -p atm-core -p atm-http-runtime --all-targets --features atm-core/test-utils -- -D warnings`
- `just lint` (boundaries, `.just/check_line_counts.py`)

## Non-closure

- No process spawn (bk-3); no `compose_storage_router`, `AtmCron` install,
  chain drain in shutdown or CLI (bk-5); no tick (bk-4).
