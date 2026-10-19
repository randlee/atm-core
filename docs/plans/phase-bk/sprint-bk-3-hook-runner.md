# Sprint bk-3 — hook runner and task integration

closure_type `boundary`; target_boundary `BOUNDARY-HookRunner` (daemon
implementation) and the router's hook consult; branch
`sprint/bk-3-hook-runner`, PR → the stack top above bk-2
(`sprint/bk-2-hook-contract`); difficulty hard. Develops in
parallel with bk-4 (disjoint owned paths), rebased onto the stack before
bk-4. Consumes bk-1 (`HookRegistrationStore`, `HookRegistration`) and
bk-2 (`HookName::parse_task_hook`, the hook request/reply types,
`HookAction::envelope`, `hook_name_for`, `ApiRequest::task_envelope`, the
sealed `HookRunner`, `CronScheduler`, `HookActionDispatcher` traits,
`LocalServiceRuntime::hook_registration_store()`, the `ATM_HOOK_*` codes,
the six protocol variants), all through `atm_core::boundary`. Governing
text: `design.md` §2.2, §3, D-1, D-3, D-6, D-8; `REQ-P-HOOK-001` §23.2,
`REQ-P-HOOK-002` §23.3, `REQ-P-CRON-001` §24.1; ADR-068 D1, D2, D4, D5,
D7, D8, ADR-069 D1, ADR-066, ADR-067.

## Scope boundary

This sprint owns the one process that runs hooks, the router code that
consults them for the six `atm task` subcommands and dispatches their
replies, the router's `HookActionDispatcher` implementation, and the daemon
side of the six registry/cron routes (hook routes call the store; cron
routes call the injected `CronScheduler`). It edits nothing under
`crates/atm`, `crates/atm-core`, `crates/atm-storage*` or `crates/atm-cron`.

Owned paths:

- `crates/atm-daemon-bootstrap/src/hook_runner.rs` (new:
  `DaemonHookRunner`), `crates/atm-daemon-bootstrap/src/process_deadline.rs`
  (new: the one spawn-with-deadline helper), `crates/atm-daemon-bootstrap/src/lib.rs`
  (edited: the two mods, `with_hook_runner` where the router is built),
  `crates/atm-daemon-bootstrap/src/received_hook_selector.rs` (edited:
  `run_tmux` at lines 458-497 calls `process_deadline::spawn_with_deadline`)
- `crates/atm-http-runtime/src/hook_dispatch.rs` (new: snapshot, consult,
  reply dispatch, `HookActionDispatcher` impl),
  `crates/atm-http-runtime/src/hook_registry_routes.rs` (new: the six
  route arms), `crates/atm-http-runtime/src/storage_and_nudge_router.rs`
  (edited: `hook_runner`, `hooks`, `cron` fields, `with_hook_runner`,
  `with_local_host`, `with_cron_scheduler`, the `hooks_disabled` flag on
  `dispatch_with_request_id` (line 1060) and `write_with_request_id`,
  consult calls in the write path before `commit_write` (line 303), in
  `task_move` (608) and in `list_messages` (93) when `task_ledger` is
  `Some`; six arms in `dispatch_non_write` (532)),
  `crates/atm-http-runtime/src/message_handler.rs` (edited: six routes
  next to `canonical_write_path()` at line 276),
  `crates/atm-http-runtime/src/lib.rs` (edited: mods)
- `docs/atm-http-runtime/openapi.yaml`, `crates/atm/openapi.yaml`,
  `crates/atm/tests/openapi_surface_baseline.json` (edited: six paths
  `/hooks/register`, `/hooks/disable`, `/hooks/list`, `/cron/start`,
  `/cron/end`, `/cron/list`, each `post`, request and response schemas from
  bk-2)
- `crates/atm-http-runtime/tests/hook_fixture/` (new: `fixture_hook.py`,
  replies selected by `ATM_HOOK_FIXTURE_REPLY` file content)
- `crates/atm-architecture/tests/process_spawn_sites.rs` (new: pins every
  `tokio::process::Command` in `crates/*/src` to the four known files)
- `boundaries/atm-core/hook-runner.toml` (edited: `[implementation]` `type
  = "DaemonHookRunner"`, `module = "atm_daemon_bootstrap::hook_runner"`,
  status `concrete_landed`),
  `boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml` (edited:
  `io_owns` + `hook_process_spawn`), `boundaries/atm-http-runtime/http-runtime.toml`
  (edited: `io_owns` + `hook_consult`, `hook_action_dispatch`)

## Public contract

```rust
// crates/atm-daemon-bootstrap/src/process_deadline.rs
pub(crate) struct SpawnedOutput { pub stdout: Vec<u8>, pub stderr: Vec<u8>, pub status: std::process::ExitStatus, pub elapsed_ms: u64 }
/// `command.kill_on_drop(true).stdin(piped).stdout(piped).stderr(piped)`; writes `stdin` and closes it; `tokio::time::timeout(remaining, child.wait_with_output())`;
/// on elapsed: `child.kill().await` then Err(on_timeout()); never retries. The only spawn-with-deadline loop in this crate (F008).
pub(crate) async fn spawn_with_deadline(command: tokio::process::Command, stdin: &[u8], remaining: Duration, on_timeout: impl FnOnce() -> AtmError) -> Result<SpawnedOutput, AtmError>;

// crates/atm-daemon-bootstrap/src/hook_runner.rs
pub struct DaemonHookRunner { observability: Arc<dyn ObservabilityPort + Send + Sync>, python: PathBuf /* "python3" resolved once */ }
impl DaemonHookRunner { pub fn new(observability: Arc<dyn ObservabilityPort + Send + Sync>) -> Self; }
impl atm_core::boundary::sealed::Sealed for DaemonHookRunner {}
#[async_trait] impl HookRunner for DaemonHookRunner {
    async fn run(&self, reg: &HookRegistration, req: &HookRequest) -> Result<HookOutcome, AtmError>;   // spawn_hook(reg, hook name, stdin = req JSON) then HookReply::parse
    async fn doctor(&self, reg: &HookRegistration) -> Result<serde_json::Value, AtmError>;              // spawn_hook(reg, "doctor", stdin = "{}") then serde_json::from_slice
    async fn stop(&self, reg: &HookRegistration, req: &HookRequest) -> Result<StopReply, AtmError>;     // spawn_hook(reg, "cron.stop", stdin = req JSON) then StopReply::parse (ADR-069 D4)
}
/// `tokio::process::Command::new(python3).arg(module).arg(sub).current_dir(cwd).env_clear()
///   .env("ATM_HOOK","1").env("ATM_TEAM",team).env("ATM_PHASE",phase).env("PATH",path).env("HOME",home)`
/// through spawn_with_deadline(cmd, stdin, Duration::from_millis(timeout_ms), || HookTimeout); non-zero status: Err(HookExit{code, stderr tail 512 bytes}).
/// The argv is exactly `[module_path, sub]`: a cron tick carries its args inside the stdin request, never on argv (ADR-069 D3).
async fn spawn_hook(&self, reg: &HookRegistration, sub: &str, stdin: &[u8]) -> Result<SpawnedOutput, AtmError>;

// crates/atm-http-runtime/src/hook_dispatch.rs
/// D-8: the in-memory view of every active registration, keyed (team, hook). Built from `store.active_all()` when the router is constructed
/// and replaced (not patched) from `active_all()` after every committed HookRegister and HookDisable. The task request path reads only this map.
pub(crate) struct ActiveHookSnapshot(std::sync::RwLock<HashMap<(TeamName, HookName), HookRegistration>>);
impl ActiveHookSnapshot { pub(crate) fn load(store: &dyn HookRegistrationStore) -> Result<Self, AtmError>; pub(crate) fn refresh(&self, store: &dyn HookRegistrationStore) -> Result<(), AtmError>; pub(crate) fn active(&self, team: &TeamName, hook: &HookName) -> Option<HookRegistration>; }
pub(crate) enum HookDecision { NoHook, PassThrough, Refused(AtmError), Actions(Vec<HookAction>) }
/// `hooks_disabled` (an action dispatch) => NoHook before any lookup (ANC-008). `hook_name_for(request)` None or no snapshot entry => NoHook (ANC-014).
/// Otherwise requires `ingress == AuthenticatedIngress::Local` (else Err(validation), like task_move at :614), builds HookRequest { caller: HookCaller { identity: request caller_identity, team: request caller_team, host: router.local_host } ,
/// request: ApiRequest::task_envelope(request) JSON }, runs `router.hook_runner.run`, logs one ADR-067 entry, maps the reply.
pub(crate) async fn consult_hook(router: &StorageAndNudgeRouter, request: &ApiRequest, ingress: AuthenticatedIngress, hooks_disabled: bool, deadline: RequestDeadline) -> Result<HookDecision, AtmError>;
/// Runs each action in order: `action.envelope()?` (bk-2 validates command ∈ TASK_HOOKS and hook_name_for == command, D-6) -> ApiRequest::from(envelope) ->
/// `router.dispatch_with_request_id_flagged(api, AuthenticatedIngress::Local, deadline, next_request_id(), hooks_disabled = true)`; stops at the first Err and returns it;
/// returns the last action's ApiResponse (the CLI prints it). One ADR-067 entry per action (REQ-BK-LOG).
pub(crate) async fn dispatch_hook_reply(router: &StorageAndNudgeRouter, hook: &HookName, actions: Vec<HookAction>, deadline: RequestDeadline) -> Result<ApiResponse, AtmError>;
impl atm_core::boundary::sealed::Sealed for StorageAndNudgeRouter {}
#[async_trait] impl HookActionDispatcher for StorageAndNudgeRouter {
    /// The cron tick's path (bk-4 calls it): same loop as dispatch_hook_reply with a fresh RequestDeadline of the daemon's default request budget,
    /// `default_as` applied where the action has no `as`, ingress Local; Ok(n) = actions run.
    async fn dispatch_actions(&self, hook: &HookName, team: &TeamName, default_as: &AgentName, actions: Vec<HookAction>) -> Result<usize, AtmError>;
}

// crates/atm-http-runtime/src/storage_and_nudge_router.rs additions
impl StorageAndNudgeRouter {
    pub fn with_hook_runner(self, runner: Arc<dyn HookRunner>) -> Self;               // default field value: Arc::new(NoopHookRunner)
    pub fn with_cron_scheduler(self, cron: Arc<dyn CronScheduler>) -> Self;           // default field value: Arc::new(NoopCronScheduler); bk-5 injects AtmCron
    /// The one dispatch entry gains a private flag; `dispatch` and `dispatch_with_request_id` (the public trait methods) pass `hooks_disabled = false`.
    fn dispatch_with_request_id_flagged(&self, request: ApiRequest, ingress: AuthenticatedIngress, deadline: RequestDeadline, request_id: RequestId, hooks_disabled: bool) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>>;
}
// Fields: hook_runner: Arc<dyn HookRunner>, hooks: ActiveHookSnapshot, cron: Arc<dyn CronScheduler>, local_host: Option<HostName> (set by with_local_host from AtmConfig.local_host via daemon_atm_config, atm_temp_config.rs:20; None when the operator has not set it).

// crates/atm-http-runtime/src/hook_registry_routes.rs
/// One function per arm. Every arm requires `ingress == AuthenticatedIngress::Local` (Err(validation) otherwise, pattern task_move :614) and that
/// `caller_identity` is a member of `caller_team` (validate_heartbeat_member, :1105). Nothing else gates them: any roster member may register a hook
/// or start a schedule (ADR-068 D5, Rand 2026-10-18 round 4: "so any roster member for both"); there is no lead check.
pub(crate) fn hook_register(router: &StorageAndNudgeRouter, req: HookRegisterRequest, ingress: AuthenticatedIngress) -> Result<ApiResponse, AtmError>;   // HookName::parse_task_hook(&req.hook) (the only validator of the name, ADR-068 D3) -> store.register -> hooks.refresh -> ResponseEnvelope::HookRegister
pub(crate) fn hook_disable(router: &StorageAndNudgeRouter, req: HookDisableRequest, ingress: AuthenticatedIngress) -> Result<ApiResponse, AtmError>;     // store.disable (rows kept, active = false) -> hooks.refresh
pub(crate) fn hook_list(router: &StorageAndNudgeRouter, query: HookListQuery, ingress: AuthenticatedIngress) -> Result<ApiResponse, AtmError>;
pub(crate) fn cron_start(router: &StorageAndNudgeRouter, req: CronStartRequest, ingress: AuthenticatedIngress) -> Result<ApiResponse, AtmError>;        // router.cron.start(NewCronSchedule { ..req fields incl. stop, created_by: caller_identity }) (parse-before-write and reload live in atm-cron)
pub(crate) fn cron_end(router: &StorageAndNudgeRouter, req: CronEndRequest, ingress: AuthenticatedIngress) -> Result<ApiResponse, AtmError>;            // router.cron.end(..)
pub(crate) fn cron_list(router: &StorageAndNudgeRouter, query: CronListQuery, ingress: AuthenticatedIngress) -> Result<ApiResponse, AtmError>;          // router.cron.list(..)
```

The store calls of the hook arms are synchronous and enter the router's
`control_path_sync_bridge` (`router_support.rs:54`, capacity 1) exactly
as `task_move` does; the cron arms call the scheduler trait, whose store
calls are the scheduler's own. The task request path makes no store call
for hooks: the snapshot is the only lookup (D-8, ANC-014).

Log entries (ADR-067 fields, `tracing::info!` like
`atm_temp_sweeper_runtime.rs:102`): `subsystem = "atm_hook"`,
`action = "run" | "action"`, `outcome = "pass_through" | "refused" | "actions" | "exit" | "timeout" | "malformed" | "action_ok" | "action_err"`,
`team`, `phase`, `hook`, `caller`, `elapsed_ms`, `action_count`,
`action_command`, `run_as`.

## Authoritative deliverables

1. `process_deadline::spawn_with_deadline` and `DaemonHookRunner` as above;
   the environment is exactly the five D-1 variables (`env_clear()` first;
   test asserts the fixture sees no other variable, REQ-BK-D1); cwd is
   `registration.cwd`; `run_tmux` in `received_hook_selector.rs` is moved
   onto the helper with its tests unchanged. `process_spawn_sites.rs`
   asserts `tokio::process::Command` appears in `crates/*/src` only in
   `atm-daemon-bootstrap/src/process_deadline.rs`,
   `atm-herdr/src/transport_cli.rs` and `atm/src/commands/send_to.rs`.
2. `ActiveHookSnapshot` (D-8): loaded once when the router is built (from
   `hook_registration_store().active_all()`), replaced after every
   committed `HookRegister` and `HookDisable`; the request path never reads
   the store (ANC-014).
3. Router consult for every task hook: `task.assign`, `task.start`,
   `task.close` in the write path before `commit_write`; `task.move` in
   `task_move`; `task.list`, `task.events` in `list_messages` when
   `query.task_ledger` is `Some`. The caller is the request's
   `caller_identity` and `caller_team` (the fields every task request
   already carries) with the router's `local_host`; a hooked request from
   any ingress but `Local` is refused with the same validation error
   `task_move` uses. `NoHook` and `PassThrough` continue into the unchanged
   existing code with the original request (ANC-005, ANC-014);
   `Refused(err)` returns the error with the hook's message and writes
   nothing (ANC-004); `Actions` returns `dispatch_hook_reply` (ANC-006).
   Every consult happens before any storage write of the request.
4. Action dispatch through the existing entry: `dispatch_with_request_id`
   gains the private `hooks_disabled` flag (no fourth public dispatch
   entry); actions pass `true` so no hook is consulted for a hook-returned
   command (ANC-008); `as` becomes the action's `caller_identity` through
   `HookAction::envelope` (ANC-009), which also refuses a `command` outside
   the six task hooks or a `request` whose `hook_name_for` is not that
   command (D-6). An action's `request` goes through the same
   `ApiRequest::from(RequestEnvelope)` as the HTTP handler; there is no
   action-specific parser (ANC-007). `impl HookActionDispatcher for
   StorageAndNudgeRouter` reuses the loop for cron ticks (bk-4) with
   ingress `Local`.
5. Registry and cron arms in `dispatch_non_write`: `HookRegister`
   (`HookName::parse_task_hook` is the only validator of the name; `cron.tick`
   and unknown names are `ATM_HOOK_MODULE_INVALID`; D-5 refusal comes from
   the store), `HookDisable`, `HookList`, `CronStart`, `CronEnd`, `CronList`
   call `router.cron` (`NoopCronScheduler` until bk-5 injects `AtmCron`). The
   six HTTP paths are added to `message_handler.rs`'s router next to
   `canonical_write_path()` (line 276) and documented in both `openapi.yaml`
   copies and the baseline. No `cron` crate, no reload
   channel and no `atm_storage` import enters `atm-http-runtime`.
6. Runner injection: `run_replacement_daemon` composes
   `DaemonHookRunner::new(observability)` and passes it with
   `with_hook_runner`; the router's defaults stay `NoopHookRunner` and
   `NoopCronScheduler`.
7. Tests, same unit (REQ-BK-TESTS: fixture module + paused clock, no live
   daemon): `tests/hook_fixture/fixture_hook.py` echoes the request JSON to
   a file and prints the reply selected by a file named in `ATM_HOOK_FIXTURE_REPLY`
   (in `cwd`, since the env is fixed); runner tests use
   the fixture for pass-through, refused, actions, non-zero exit, malformed
   stdout, a `sleep` beyond `timeout_ms`, `doctor` and both `cron.stop`
   replies (`tokio::time::pause` for the deadline, a real child for the
   kill); router tests use `DummyHookRunner` and
   `InMemoryHookRegistrationStore` (from `atm-runtime-test-support`)
   through the existing router test harness
   (`storage_and_nudge_router.rs:1563` pattern) for each of the six hooks,
   plus `RecordingCronScheduler` for the three cron arms.
   `.just/check_line_counts.py` passes: new code lives in
   `hook_dispatch.rs` and `hook_registry_routes.rs`.

## Authoritative acceptance criteria

- [ ] (ANC-001) `boundary:HookRunner` a `task.assign` request with an active hook reaches the runner before any task row exists; the test asserts `DummyHookRunner.calls.len() == 1` and the ledger is unchanged when the reply is `Refused`.
- [ ] (ANC-004) `boundary:HookRunner` on `{"success":false,"message":"bd not ready"}` the write path returns `AtmError` with message `bd not ready` and `open_tasks` is empty.
- [ ] (ANC-005) `boundary:HookRunner` on `{"success":true}` the resulting `TaskRow` equals the row produced with no registration, field by field.
- [ ] (ANC-006, ANC-007) `boundary:HookRunner` on an `atm-action` list of two `task.assign` envelopes the ledger holds both tasks in list order and the original request's task id is absent; an action whose `command` is `task.close` but whose `request` is a `task.assign` envelope fails with `ATM_HOOK_MALFORMED` and writes nothing.
- [ ] (ANC-008) `boundary:HookRunner` with a `task.assign` hook registered and an action list containing a `task.assign`, `DummyHookRunner.calls.len() == 1` after dispatch, and `dispatch_with_request_id` (the public entry) is the only path the action took; no new public dispatch function exists.
- [ ] (ANC-021) `boundary:HookRunner` `dispatch_actions` (the `HookActionDispatcher` impl) and the task-hook `Actions` path call the same `dispatch_hook_reply`; a test through the trait object yields the same ledger rows and log entries as the task-hook path, and no second action path exists.
- [ ] (ANC-009) `boundary:HookRunner` an action with `"as":"team-lead"` produces a `task_events` row whose actor is `team-lead`; without `as` the actor is the caller.
- [ ] (ANC-012) `boundary:HookRunner` a fixture that sleeps 5 s under `timeout_ms = 200` returns `ATM_HOOK_TIMEOUT` and the child is gone (`try_wait` is `Some`) within the test.
- [ ] (REQ-BK-ERRORS) `boundary:HookRunner` fixture exit 3 → `ATM_HOOK_EXIT` with `exit 3` in the message; fixture printing `not json` → `ATM_HOOK_MALFORMED`; neither writes a task row.
- [ ] (ANC-013) `boundary:HookRunner` six router tests, one per hook name, each registering only that hook and asserting the runner is called for that subcommand and not for the other five.
- [ ] (ANC-014) `boundary:HookRunner` with no registration, the router's write, move and list paths make no `hook_registration` store call (`InMemoryHookRegistrationStore` call counter is 0 after construction); the existing task CLI integration tests pass unchanged.
- [ ] (D-8) `boundary:HookRunner` after `HookRegister` commits, the next `task.close` consults the runner without a restart; after `HookDisable` it does not; the store counter rises only on the two registry calls.
- [ ] (REQ-BK-D1) `boundary:HookRunner` the fixture dumps `os.environ`; the test asserts the key set is exactly `{ATM_HOOK, ATM_TEAM, ATM_PHASE, PATH, HOME}` and `os.getcwd()` equals the registered cwd.
- [ ] (REQ-BK-D3) `boundary:HookRunner` `run_replacement_daemon` builds one `DaemonHookRunner` and passes it to the router with `with_hook_runner`; the `atm` CLI spawns no hook process: its only `tokio::process::Command` is the pre-existing Send-To transfer in `send_to.rs`, and `process_spawn_sites.rs` pins the three spawn files.
- [ ] (REQ-BK-LOG) `boundary:HookRunner` a run with two actions produces three `atm_hook` log entries (one `run`, two `action`) carrying `team`, `phase`, `hook`, `outcome`, `elapsed_ms`, captured with the observability test port.
- [ ] (ANC-022) `boundary:HookRunner` the request the fixture receives carries `custom` byte-identical to the registration-time value (empty object in bk).
- [ ] (ANC-002) `boundary:HookRunner` the `HookDisable` arm leaves the rows present with `active == false` and `disabled_at` set, and `HookList` still returns them.
- [ ] (ANC-003) `boundary:HookRunner` for each of the six hooks the runner receives a `HookRequest` whose `caller.identity`/`caller.team` equal the request's `caller_identity`/`caller_team` and whose `request` is the serde JSON of the `RequestEnvelope` the CLI sent.
- [ ] (ANC-010) `boundary:HookRunner` the hook-module spawn in `hook_runner.rs` is the only subprocess this sprint adds; `hook_dispatch.rs`, `hook_registry_routes.rs` and `hook_runner.rs` never call `bd` or read or write `.beads`; every bd call happens inside the spawned hook process.
- [ ] (ADR-068 D3 doctor entry) `boundary:HookRunner` `DaemonHookRunner::doctor` on a fixture whose `doctor()` prints `{"manifest":"ok"}` returns that value; on a fixture that sleeps past `timeout_ms` it returns `ATM_HOOK_TIMEOUT`.
- [ ] (ADR-069 D1) `boundary:HookRunner` the `CronStart` arm passes `NewCronSchedule { team: caller_team, phase, name, cron_expr, module_path, cwd, args, timeout_ms, created_by: caller_identity }` to `CronScheduler::start` and returns its row as `ResponseEnvelope::CronStart`; a scheduler `Err(CronExprInvalid)` is returned unchanged; the arm itself parses nothing.
- [ ] (ADR-069 D1) `boundary:HookRunner` `CronEnd` returns the rows the scheduler ended; `CronList` returns `scheduler.list(Some(caller_team), phase)`; each of the six routes answers on its path with its bk-2 response variant.
- [ ] (ADR-068 D5 authority) `boundary:HookRunner` `HookRegister` and `CronStart` from any roster member of the team succeed (a non-lead member in the test roster); from an identity not on the roster they fail with the agent-not-found error; no route reads a roster role.
- [ ] (ADR-069 D4) `boundary:HookRunner` the `CronStart` arm passes `req.stop` through unchanged to `CronScheduler::start`; the openapi `CronStartRequest` schema marks `stop` required with the two `kind` values `until` and `hook`.
- [ ] (ADR-069 D4) `boundary:HookRunner` `DaemonHookRunner::stop` on a fixture whose `stop()` prints `{"stop": true, "reason": "phase bead closed"}` returns that `StopReply`; on `{"stop": false}` returns `stop == false`; on `{"stop": true}` returns `ATM_HOOK_MALFORMED`; the fixture's `sys.argv[1:] == ["cron.stop"]` and its stdin carries `hook == "cron.stop"`.
- [ ] (ADR-068 D3) `boundary:HookRunner` `HookRegister` with `hook = "cron.tick"` or `"task.foo"` returns `ATM_HOOK_MODULE_INVALID`; the route is the only validator (`HookName::parse_task_hook` has exactly one call site in `atm-http-runtime`).
- [ ] `boundary:HookRunner` a hooked `task.close` from `AuthenticatedIngress::Peer` is refused before the runner is called (`DummyHookRunner.calls.len() == 0`).

## Authoritative validation

- `cargo test -p atm-daemon-bootstrap -p atm-http-runtime -p atm-architecture`
- `cargo clippy -p atm-daemon-bootstrap -p atm-http-runtime --all-targets -- -D warnings`
- `cargo build --workspace`
- `just lint`
- `python3 .just/check_line_counts.py`

## Non-closure

- No cron tick or stop check runs (bk-4 builds those requests and uses
  the runner's `run`/`stop` and `HookActionDispatcher`).
- No CLI command sends the registry or cron envelopes (bk-5); no
  `TIMEOUT_MS` is read (bk-5); `HookRunner::doctor` is implemented here but
  called only by bk-5's doctor section.
