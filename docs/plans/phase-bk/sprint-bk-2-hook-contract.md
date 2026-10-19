# Sprint bk-2 — core hook contract and sealed traits

closure_type `contract`; target_boundary `atm_core::hook` and the four
sealed `atm_core::boundary` traits `TaskCommands`, `HookRunner`,
`CronScheduler`, `HookActionDispatcher` with their doubles; branch
`sprint/bk-2-hook-contract`, PR → `sprint/bk-1-hook-stores` (layer 2).
Consumes from bk-1 the hook and cron store types, the in-memory doubles,
the budget constants and the error constructors. Governing text:
`design.md` §2, §9.2-§9.5, §9.11 (inlined below). Requirements `REQ-CORE-HOOK-001` §23.4, `REQ-P-HOOK-001` §23.2,
`REQ-P-HOOK-004` §11.5 (the doctor request and reply types); ADR-068 D1,
D2, D3, D8, ADR-069 D2, D4.

## Scope boundary

Every type and trait the runtime sprints consume; nothing that runs a
process, serves a request or touches SQLite. No envelope or `ApiRequest`
variant and no route: those change the only exhaustive matches
(`message_handler.rs:707`, `storage_and_nudge_router.rs:532`), so they land
with those arms in bk-7 and every layer builds.

Owned paths (new unless marked edited):

- `crates/atm-core/src/hook/mod.rs`, `crates/atm-core/src/hook/request.rs`,
  `crates/atm-core/src/hook/reply.rs`, `crates/atm-core/src/hook/routes.rs`
  (the six request structs and `HookActionsResponse`),
  `crates/atm-core/src/lib.rs` (edited: `pub mod hook`)
- `crates/atm-core/src/boundary/task_commands.rs`,
  `crates/atm-core/src/boundary/hook_runner.rs`,
  `crates/atm-core/src/boundary/cron_scheduler.rs`,
  `crates/atm-core/src/boundary/hook_action_dispatcher.rs`,
  `crates/atm-core/src/boundary/mod.rs` (edited: four `mod` lines after
  `:86` and the re-exports; 235 production lines)
- `crates/atm-core/src/api.rs` (edited: on `RequestDeadline` (`:760`)
  `PartialEq, Eq`, `remaining_ms`, `minus`; 580 production lines)
- `crates/atm-core/src/service_runtime.rs` (edited: two fields, builders
  and accessors next to `task_store` (`:325`, `:506`, `:515`); 774
  production lines)
- `crates/atm-core/src/doctor/hooks.rs`, `crates/atm-core/src/doctor/mod.rs`
  (edited: `pub mod hooks`)
- `boundaries/atm-core/task-commands.toml` (`BOUNDARY-TaskCommands`),
  `boundaries/atm-core/hook-runner.toml` (`BOUNDARY-HookRunner`),
  `boundaries/atm-core/cron-scheduler.toml` (`BOUNDARY-CronSchedulerTrait`),
  `boundaries/atm-core/hook-action-dispatcher.toml`
  (`BOUNDARY-HookActionDispatcher`); `visibility = "trait_only"`,
  `constructor = "none"`, format of `message-received-hook-emitter.toml`
- `crates/atm-runtime-test-support/src/lib.rs` (edited: re-exports of the
  two in-memory stores and the four core doubles; 361 production lines)
- `crates/atm-architecture/tests/hook_trait_boundary.rs`

## Public contract

```rust
// crates/atm-core/src/hook/request.rs  (design.md §2.1, ANC-003, ANC-022)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HookCaller { pub identity: AgentName, pub team: TeamName, pub host: Option<HostName> }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookRequest {
    pub schema_version: String,     // HOOK_SCHEMA_VERSION
    pub hook: HookName, pub team: TeamName, pub phase: PhaseId, pub caller: HookCaller,
    pub request: serde_json::Value, // the bare HTTP body (TaskCommand::request_body); {"args": [...], "created_by": "<agent>"} for cron.tick and cron.stop
    pub custom: serde_json::Map<String, serde_json::Value>,  // the registration's (or schedule's) stored custom, unchanged (Rand 2026-10-18)
}
pub const HOOK_SCHEMA_VERSION: &str = "1.0.0";   // independent of HTTP_API_VERSION (ADR-068 D8)
pub const HOOK_CUSTOM_MAX_BYTES: usize = 65_536;  pub const HOOK_PATH_MAX_BYTES: usize = 4096;
pub const CRON_NAME_MAX_BYTES: usize = 64;        pub const CRON_ARGS_MAX: usize = 32;  pub const CRON_ARGS_MAX_BYTES: usize = 4096;
/// A non-object or a serialized size above HOOK_CUSTOM_MAX_BYTES is hook_module_invalid naming `custom` and the limit.
pub fn custom_object(value: serde_json::Value) -> Result<serde_json::Map<String, serde_json::Value>, AtmError>;
#[derive(Clone, Debug, PartialEq)]
pub struct HookTarget { pub team: TeamName, pub phase: PhaseId, pub module_path: PathBuf, pub cwd: PathBuf, pub timeout_ms: u32, pub custom: serde_json::Map<String, serde_json::Value> }
impl From<&HookRegistration> for HookTarget {}  impl From<&CronSchedule> for HookTarget {}
impl HookRequest {
    pub fn for_task(target: &HookTarget, hook: HookName, caller: HookCaller, command: &TaskCommand) -> Self;   // custom = target.custom
    pub fn for_cron(target: &HookTarget, caller: HookCaller, hook: HookName, args: &[String]) -> Self;        // request.created_by = caller.identity
}

// crates/atm-core/src/hook/reply.rs
pub enum HookReply { Handled(Vec<HookAction>), PassThrough, NotApplicable, Refused { message: String } }   // no serde derive
pub const HOOK_ACTIONS_MAX: usize = 32;
impl HookReply { pub fn parse(stdout: &[u8]) -> Result<Self, AtmError>; }   // table below
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookAction { pub command: String, #[serde(rename = "as", default)] pub run_as: Option<AgentName>, pub request: serde_json::Value }
impl HookAction { pub fn into_api_request(&self, team: &TeamName, default_as: &AgentName) -> Result<ApiRequest, AtmError>; }   // action table below
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopReply { pub stop: bool, #[serde(default)] pub reason: Option<String> }
impl StopReply { pub fn parse(stdout: &[u8]) -> Result<Self, AtmError>; }   // exactly {"stop":true,"reason":"<non-empty>"} | {"stop":false}

// crates/atm-core/src/hook/routes.rs  (design.md §9.11 request fields; bk-7 adds the envelope variants and routes)
pub struct HookRegisterRequest { pub caller_identity: AgentName, pub caller_team: TeamName, pub phase: PhaseId, pub hook: String,
    pub module_path: PathBuf, pub cwd: PathBuf, pub timeout_ms: u32, #[serde(default)] pub custom: serde_json::Value }
pub struct HookDisableRequest { pub caller_identity: AgentName, pub caller_team: TeamName, pub phase: PhaseId, pub hook: Option<String> }
pub struct HookListQuery { pub caller_identity: AgentName, pub caller_team: TeamName, pub phase: Option<PhaseId> }
pub struct CronStartRequest { pub caller_identity: AgentName, pub caller_team: TeamName, pub phase: PhaseId, pub name: String, pub cron_expr: String,
    pub module_path: PathBuf, pub cwd: PathBuf, pub args: Vec<String>, pub timeout_ms: u32, pub stop: StopCondition, #[serde(default)] pub custom: serde_json::Value }
pub struct CronEndRequest { pub caller_identity: AgentName, pub caller_team: TeamName, pub phase: PhaseId, pub name: Option<String> }
pub struct CronListQuery { pub caller_identity: AgentName, pub caller_team: TeamName, pub phase: Option<PhaseId> }
pub struct HookActionsResponse { pub dispatched: usize }   // design D-b; all derive Clone, Debug, PartialEq, Serialize, Deserialize
impl HookRegisterRequest { pub fn check_limits(&self) -> Result<serde_json::Map<String, serde_json::Value>, AtmError>; }
impl CronStartRequest { pub fn check_limits(&self) -> Result<serde_json::Map<String, serde_json::Value>, AtmError>; }
// check_limits: custom_object(custom), module_path and cwd at most HOOK_PATH_MAX_BYTES, and for cron name 1..=CRON_NAME_MAX_BYTES,
// at most CRON_ARGS_MAX args of CRON_ARGS_MAX_BYTES total; each breach is hook_module_invalid naming the field and limit. Ok is the custom map.

// crates/atm-core/src/api.rs (edited :760)
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct RequestDeadline(Instant);
impl RequestDeadline { pub fn remaining_ms(self) -> u64;              // remaining() in whole ms, 0 when expired
                       pub fn minus(self, d: Duration) -> Self; }     // the instant d earlier (checked_sub; self when not representable)

// crates/atm-core/src/boundary/task_commands.rs  (design.md §9.5)
pub struct TaskCallCtx {
    pub caller_identity: AgentName, pub caller_team: TeamName, pub host: Option<HostName>,
    pub ingress: AuthenticatedIngress, pub deadline: RequestDeadline, pub request_id: RequestId,
}
pub struct TaskAssign(pub Box<WriteRequest>);  pub struct TaskStart(pub Box<WriteRequest>);  pub struct TaskClose(pub Box<WriteRequest>);
pub struct TaskList(pub ListQuery);            pub struct TaskEvents(pub ListQuery);         // move_task takes TaskMoveRequest
#[async_trait]
pub trait TaskCommands: sealed::Sealed + Send + Sync {
    async fn assign(&self, req: TaskAssign, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn start(&self, req: TaskStart, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn close(&self, req: TaskClose, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn move_task(&self, req: TaskMoveRequest, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn list(&self, req: TaskList, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn events(&self, req: TaskEvents, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
}
pub enum TaskCommand { Assign(TaskAssign), Start(TaskStart), Close(TaskClose), Move(TaskMoveRequest), List(TaskList), Events(TaskEvents) }
impl TaskCommand {
    pub fn classify(request: ApiRequest) -> Result<Self, ApiRequest>;   // classify table below; the request back when not a task command
    pub fn hook_name(&self) -> HookName;  pub fn request_body(&self) -> serde_json::Value;
    pub async fn call(self, commands: &dyn TaskCommands, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
}
#[cfg(any(test, feature = "test-utils"))]
pub struct RecordingTaskCommands { pub calls: Mutex<Vec<(HookName, serde_json::Value)>>, pub response: Mutex<Option<ApiResponse>> }

// crates/atm-core/src/boundary/hook_runner.rs  (design.md D-3, §9.7)
#[derive(Debug)] pub struct HookOutcome { pub reply: HookReply, pub elapsed_ms: u64 }
#[async_trait]
pub trait HookRunner: sealed::Sealed + Send + Sync {
    /// argv[1] = request.hook; one spawn with `request` on stdin; Err is HookExit, HookTimeout or HookMalformed.
    async fn run(&self, target: &HookTarget, request: &HookRequest, deadline: RequestDeadline) -> Result<HookOutcome, AtmError>;
    /// argv[1] "doctor", stdin HookDoctorRequest::for_target(target); Ok is the parsed HookDoctorReply.
    async fn doctor(&self, target: &HookTarget, deadline: RequestDeadline) -> Result<HookDoctorReply, AtmError>;
    async fn stop(&self, target: &HookTarget, request: &HookRequest, deadline: RequestDeadline) -> Result<StopReply, AtmError>;
}
#[cfg(any(test, feature = "test-utils"))] #[derive(Clone, Debug)] pub enum HookCall { Run(HookRequest), Stop(HookRequest), Doctor }
// each call records (target, call, deadline), awaits before_reply(index) when set (the test's own watch future: no sleep, no tokio in atm-core), then replies
#[cfg(any(test, feature = "test-utils"))] pub type BeforeReply = Arc<dyn Fn(usize) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;
#[cfg(any(test, feature = "test-utils"))]
pub struct DummyHookRunner {
    pub replies: Mutex<VecDeque<Result<HookOutcome, AtmError>>>,
    pub stop_replies: Mutex<VecDeque<Result<StopReply, AtmError>>>,
    pub doctor_replies: Mutex<VecDeque<Result<HookDoctorReply, AtmError>>>,
    pub calls: Mutex<Vec<(HookTarget, HookCall, RequestDeadline)>>,
    pub before_reply: Mutex<Option<BeforeReply>>,
    pub panic_on_run: AtomicBool,
}

// crates/atm-core/src/boundary/cron_scheduler.rs  (design.md D-4: start, end, list only)
#[async_trait]
pub trait CronScheduler: sealed::Sealed + Send + Sync {
    async fn start(&self, spec: CronStartSpec) -> Result<CronSchedule, AtmError>;
    async fn end(&self, team: &TeamName, phase: &PhaseId, name: Option<&str>) -> Result<Vec<CronSchedule>, AtmError>;
    async fn list(&self, team: Option<&TeamName>, phase: Option<&PhaseId>) -> Result<Vec<CronSchedule>, AtmError>;
}
#[derive(Clone, Debug, PartialEq)]
pub struct CronStartSpec {
    pub team: TeamName, pub phase: PhaseId, pub name: String, pub cron_expr: String,
    pub module_path: PathBuf, pub cwd: PathBuf, pub args: Vec<String>, pub timeout_ms: u32,
    pub custom: serde_json::Map<String, serde_json::Value>, pub stop: StopCondition, pub created_by: AgentName,
}
#[cfg(any(test, feature = "test-utils"))] pub struct RecordingCronScheduler { pub rows: Mutex<Vec<CronSchedule>>, pub calls: Mutex<Vec<&'static str>> }

// crates/atm-core/src/boundary/hook_action_dispatcher.rs  (implemented by bk-7's UnhookedRouter; called by bk-7's filter and atm-cron)
#[derive(Clone, Debug, PartialEq)]
pub struct DispatchSpec { pub hook: HookName, pub team: TeamName, pub phase: PhaseId, pub default_as: AgentName, pub deadline: Option<RequestDeadline>, pub dry_run: bool }
#[async_trait]
pub trait HookActionDispatcher: sealed::Sealed + Send + Sync {
    /// Checks every action first (design §2.2: caller_team == team, `as` on the roster, into_api_request(team, default_as) Ok), else
    /// HookMalformed and nothing runs. Then runs them in order un-hooked as `default_as` unless `as`; deadline Some is reused, None gives
    /// each action now + SERVER_REQUEST_BUDGET. Stops at the first Err with the design §2.2 prefix; Ok holds every response in order.
    /// dry_run: check only, Ok(vec![]).
    async fn dispatch_actions(&self, spec: DispatchSpec, actions: Vec<HookAction>) -> Result<Vec<(String, ApiResponse)>, AtmError>;
}
#[cfg(any(test, feature = "test-utils"))] #[derive(Clone, Debug)] pub struct DispatchCall { pub spec: DispatchSpec, pub actions: Vec<HookAction> }
#[cfg(any(test, feature = "test-utils"))] pub struct RecordingHookActionDispatcher { pub calls: Mutex<Vec<DispatchCall>>,
    pub fail_at: Option<usize>, pub panic_at: Option<usize> }

// crates/atm-core/src/boundary/mod.rs: mod task_commands; mod hook_runner; mod cron_scheduler; mod hook_action_dispatcher; then
pub use atm_storage::{HookRegistrationStore, HookRegistration, HookName, PhaseId, CronScheduleStore, CronSchedule, NewCronSchedule, CronScheduleId, StopCondition, HookExitCause};
pub use atm_storage::request_budget::{HOOK_TIMEOUT_MAX_MS, CRON_TIMEOUT_MAX_MS, HOOK_ACTION_RESERVE_MS, SERVER_REQUEST_BUDGET};
pub use crate::hook::HookTarget; pub use task_commands::*; pub use cron_scheduler::{CronScheduler, CronStartSpec};
pub use hook_action_dispatcher::{DispatchSpec, HookActionDispatcher}; pub use hook_runner::{HookRunner, HookOutcome};
#[cfg(any(test, feature = "test-utils"))] pub use {cron_scheduler::RecordingCronScheduler, hook_action_dispatcher::{DispatchCall, RecordingHookActionDispatcher}, hook_runner::{BeforeReply, DummyHookRunner, HookCall}};

// crates/atm-core/src/doctor/hooks.rs  (design.md §9.10; Rand 2026-10-18: "doctor would need to know team, phase and it would return a dict/array")
#[serde(deny_unknown_fields)] pub struct HookDoctorRequest { pub schema_version: String, pub hook: String /* "doctor" */, pub team: TeamName, pub phase: PhaseId, pub custom: serde_json::Map<String, serde_json::Value> }
impl HookDoctorRequest { pub fn for_target(target: &HookTarget) -> Self; }
#[serde(deny_unknown_fields)] pub struct HookDoctorReply { pub success: bool, pub message: String, #[serde(default)] pub findings: Vec<HookDoctorFinding> }
impl HookDoctorReply { pub fn parse(stdout: &[u8]) -> Result<Self, AtmError>; }   // any other shape: hook_malformed
/// The fields of DoctorFinding (report.rs:38); `code` is the module's own string, not an AtmErrorCode.
#[serde(deny_unknown_fields)] pub struct HookDoctorFinding { pub severity: DoctorSeverity, pub code: String, pub message: String, #[serde(default)] pub remediation: Option<String> }
pub struct HookDoctorEntry {
    pub module_path: PathBuf, pub cwd: PathBuf, pub team: TeamName, pub phase: PhaseId,
    pub hooks: Vec<HookName>, pub schedules: Vec<String>, pub timeout_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub message: Option<String>,
    #[serde(default)] pub findings: Vec<HookDoctorFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub error: Option<HookDoctorError>,
}
pub struct HookDoctorError { pub code: AtmErrorCode, pub message: String }   // all derive Clone, Debug, PartialEq, Eq, Serialize, Deserialize

// crates/atm-core/src/service_runtime.rs, pattern task_store :506/:515
pub fn with_hook_registration_store(self, store: Arc<dyn HookRegistrationStore>) -> Self;
pub fn hook_registration_store(&self) -> Result<Arc<dyn HookRegistrationStore>, AtmError>;   // not installed: the task_store error shape naming the store
pub fn with_cron_schedule_store(self, store: Arc<dyn CronScheduleStore>) -> Self;
pub fn cron_schedule_store(&self) -> Result<Arc<dyn CronScheduleStore>, AtmError>;
```

`HookReply::parse` (§9.4): `Handled` is an object with keys exactly
`{result, "atm-action"}`, `result` `"handled"` and `atm-action` an array of
1 to `HOOK_ACTIONS_MAX` valid `HookAction`; `PassThrough` keys exactly
`{result}` with `"pass-through"`; `NotApplicable` keys exactly `{result}`
with `"not-applicable"`; `Refused` keys exactly `{result, message}` with
`"refused"` and a non-empty message. Every other stdout is
`ATM_HOOK_MALFORMED`, including `{"result":"handled","atm-action":{}}`,
`..:[]`, `..:[1]`, 33 actions, `{"result":"handled"}`,
`{"result":"pass-through","extra":1}`,
`{"result":"not-applicable","message":"x"}`,
`{"result":"refused","message":""}`, `{"result":"refused"}`,
`{"result":"skip"}`, `{"success":true}` (the old shape), two objects,
empty, `null` and truncated JSON.

`HookAction::into_api_request` (§9.2) deserializes `request` with the
serde type the HTTP route for `command` deserializes (the bare HTTP body),
checks the condition and team field, and sets the
identity field from `as`, else `default_as`; the body's own identity never
stands. `send`: `WriteRequest` with no `task_id`, `task_op`,
`task_complete`, `acknowledges_message_id`. `ack`: `WriteRequest` with
`acknowledges_message_id`. `task.assign`/`task.start`/`task.close`:
`WriteRequest` that `TaskCommand::classify` yields as that method.
`task.move`: `TaskMoveRequest`. `task.list`/`task.events`/`task.history`:
`ListQuery` with `task_ledger` `Tasks`/`Events`/`History`. `read`:
`ReadQuery`; `peek`: `PeekQuery`; `clear`: `ClearQuery`; for all of these
the team field is `caller_team` and the identity field `caller_identity`.
`search`: `SearchRequest`, `as` absent, `query.team` absent or the
registration team and forced to it. `doctor`: `DoctorQuery`, `as` absent,
`all_teams` false, `team_override` absent or the registration team and
forced to it. A `caller_team` other than the registration team, the six
`hook.*`/`cron.*` registry commands, heartbeat, graft, reload,
compatibility, queue and any other name are `ATM_HOOK_MALFORMED` (D-6).

`TaskCommand::classify` (§9.3): `Write` with `task_op == Some(Start)` is
`task.start`; `task_op == Some(Close)`, or legacy `task_complete` with no
`task_op`, is `task.close`; `task_id` `Some` with `task_op`,
`task_complete`, `acknowledges_message_id` all `None` is `task.assign`; a
`Write` whose `task_op_normalized()` is `Err`, or with
`acknowledges_message_id`, is not a task command (the existing validation
error surfaces); `TaskMove` is `task.move`; `Messages(List)` with
`task_ledger` `Tasks` is `task.list`, `Events` is `task.events`,
`History` is not a task command (D-h); everything else is not.

## Authoritative deliverables

1. `atm-core::hook`: the types above. `parse` is the only reply parser; a
   reply that is not one of the four shapes is `ATM_HOOK_MALFORMED` to the
   caller, never pass-through or the normal command (Rand, 2026-10-18:
   "manformed hook MUST return hook failed to caller (error)").
2. `atm-core::boundary`: the four sealed traits, `test-utils` doubles,
   `TaskCommand`, `DispatchSpec` and the re-exports, so no crate but
   `atm-core` imports `atm_storage` for hook or cron types. No `Noop` types.
3. The six request structs, `HookActionsResponse`, `check_limits`.
4. `RequestDeadline` additions; the `LocalServiceRuntime` store accessors.
5. The doctor request, reply, finding and entry types.
6. Boundary manifests: `allowed_test_double_paths` in the defining-module
   form (`["crate::boundary::hook_runner::DummyHookRunner"]` and likewise);
   `allowed_dependents`: `task-commands.toml` `["atm-daemon-bootstrap",
   "atm-http-runtime", "atm-runtime-test-support"]`; `hook-runner.toml`
   `["atm", "atm-cron", "atm-daemon-bootstrap", "atm-http-runtime",
   "atm-runtime-test-support"]`; `cron-scheduler.toml` and
   `hook-action-dispatcher.toml` `["atm-cron", "atm-daemon-bootstrap",
   "atm-http-runtime", "atm-runtime-test-support"]`. Later sprints edit only
   the `[status]` table of these four.
7. Tests, same unit, in the files the criteria name.

## Authoritative acceptance criteria

- [ ] (ANC-003) `boundary:BOUNDARY-HookRunner` `crates/atm-core/src/hook/request.rs::tests::hook_request_top_level_keys_and_request_shape_are_pinned`: for the six task commands and both cron hooks the keys are `schema_version, hook, team, phase, caller, request, custom`, `caller` keys `identity, team, host`, task `request` equals the CLI envelope's `encode_request_body`, cron `request` is `{"args": [...], "created_by": <caller.identity>}`.
- [ ] (ANC-022) `boundary:BOUNDARY-HookRunner` `request.rs::tests::custom_round_trips_and_comes_from_the_registration` (`{"x":[1,{"y":null}]}` byte-identical; `for_task` and `for_cron` copy `target.custom`); `::custom_object_refuses_non_object_and_oversize` (`[1]`, `"s"`, 65 537 bytes refused naming `custom`; 65 536 accepted).
- [ ] (service limits) `boundary:BOUNDARY-HookRunner` `crates/atm-core/src/hook/routes.rs::tests::check_limits_refuses_each_oversize_field`: a 65-byte cron `name`, an empty one, 33 args, 4 097 arg bytes, and a 4 097-byte `module_path` or `cwd` are each `ATM_HOOK_MODULE_INVALID` naming the field; values at each limit pass.
- [ ] (ANC-013) `boundary:BOUNDARY-TaskCommands` `crates/atm-core/src/boundary/task_commands.rs::tests::classify_maps_each_task_request` asserts every classify rule above and each `hook_name()`; `::normalization_error_surfaces_the_existing_validation_error`.
- [ ] (ANC-007, ANC-009) `boundary:BOUNDARY-HookRunner` `crates/atm-core/src/hook/reply.rs::tests::action_maps_each_command`: one action per command yields the named `ApiRequest`; `as` sets the identity field, else `default_as` even when the body names another; another `caller_team` is malformed; `search`/`doctor` force the team, and `as` on either, `all_teams: true`, or a foreign team are malformed; the CLI's `atm task assign` HTTP body, read with the route's type, gives the handler's request.
- [ ] (D-6) `boundary:BOUNDARY-HookRunner` `reply.rs::tests::registry_cron_and_internal_commands_are_malformed`: `hook.register`, `cron.start`, `heartbeat`, `task.foo`, and `send` with a `task_id` fail with `ATM_HOOK_MALFORMED`; a key other than `command`, `as`, `request` is refused.
- [ ] (ANC-004, ANC-005, ANC-006) `boundary:BOUNDARY-HookRunner` `reply.rs::tests::reply_parse_case_table`: the four valid shapes map to `Handled` (order kept), `PassThrough`, `NotApplicable`, `Refused`; every malformed stdout listed above is `ATM_HOOK_MALFORMED`; 32 actions parse and 33 do not; `malformed_is_never_pass_through`: no malformed row parses to `Ok(PassThrough)` or `Ok(NotApplicable)`.
- [ ] (ANC-024) `boundary:BOUNDARY-HookRunner` `reply.rs::tests::stop_reply_shapes`: accepts exactly `{"stop": true, "reason": "r"}` and `{"stop": false}`; `{"stop": true}`, `{"stop": true, "reason": ""}`, `{"stop": false, "reason": "x"}`, `{"stop": "yes"}`, `{"stop":1}` and an extra key are `ATM_HOOK_MALFORMED`.
- [ ] (REQ-BK-D3) `boundary:BOUNDARY-HookRunner` `HookRunner` is the one runner trait (`&HookTarget` on every method: task hooks, cron and doctor); the four traits are sealed, their doubles compile only under `test-utils`; `just lint` passes the four manifests.
- [ ] (REQ-BK-D3) `boundary:BOUNDARY-HookRunner` `crates/atm-architecture/tests/hook_trait_boundary.rs`: `::bk_hook_and_cron_types_reach_runtime_crates_only_through_atm_core_boundary` (a `syn` visitor over the non-test `src` of `atm-http-runtime`, `atm-daemon-bootstrap`, `atm` and `atm-cron` finds no `atm_storage::` path naming a re-exported type or constant; a crate not yet created is skipped, and bk-4's validation runs it with `atm-cron` present); `::bk_task_commands_has_only_known_implementations`, `::bk_hook_runner_..`, `::bk_cron_scheduler_..`, `::bk_hook_action_dispatcher_..`: over every existing `crates/*/src`, the last path-segment ident of each `impl <Trait> for <T>` (generic arguments ignored) is in `{NormalTaskCommands, HookedTaskCommands, RecordingTaskCommands}`, `{DaemonHookRunner, DummyHookRunner}`, `{AtmCron, RecordingCronScheduler}` and `{UnhookedRouter, RecordingHookActionDispatcher}`.
- [ ] (REQ-BK-TESTS) `boundary:BOUNDARY-AtmProtocol` `crates/atm-core/src/api.rs::tests::request_deadline_equality_minus_and_remaining_ms`: `d = after(10 s)`: `d.minus(4 s) == d.minus(1 s).minus(3 s)`, `d != d.minus(1 ms)`, `after(ZERO).remaining_ms() == 0`, `d.remaining_ms() <= 10_000`, `d.minus(20 s).remaining_ms() == 0`.
- [ ] (ANC-023) `boundary:BOUNDARY-AtmProtocol` `routes.rs::tests::request_structs_round_trip`: each of the six structs round-trips through serde; a `CronStartRequest` without `stop` fails to deserialize and `{"kind":"until","at":"2026-11-01T00:00:00Z"}` gives `StopCondition::Until`; absent `custom` is `null`, which `custom_object` refuses and the route maps to `{}` (bk-7).
- [ ] `boundary:BOUNDARY-AtmRuntime-Composition` `crates/atm-core/src/service_runtime.rs::tests::hook_and_cron_stores_report_the_not_installed_error` (pattern `:1264`) and `::installed_hook_and_cron_stores_are_returned` (in-memory doubles).
- [ ] (ANC-015, REQ-P-HOOK-004) `boundary:BOUNDARY-DaemonRuntimeDoctorReport` `crates/atm-core/src/doctor/hooks.rs::tests::doctor_request_and_reply_shapes`: `for_target` serializes exactly `schema_version, hook, team, phase, custom` (`hook == "doctor"`); `parse` accepts `{"success":true,"message":"ok"}` (`findings == []`) and the §9.10 example; a missing `success` or `message`, an extra key, `"severity":"fatal"`, a finding without `code`, `findings: {}` and `ok` are `ATM_HOOK_MALFORMED`; `HookDoctorFinding` has `DoctorFinding`'s four field names.

## Authoritative validation

- `cargo build --workspace`
- `cargo test -p atm-core --features test-utils -p atm-runtime-test-support -p atm-architecture --test hook_trait_boundary`
- `cargo clippy -p atm-core -p atm-runtime-test-support --all-targets --features atm-core/test-utils -- -D warnings`
- `just lint` (boundary manifests, `.just/check_line_counts.py`)

## Non-closure

- No envelope variant, route, `HTTP_API_VERSION` bump, `openapi.yaml` edit
  or `DoctorReport.hooks` field (bk-7); no process spawned (bk-3); no
  `TaskCommands` or dispatcher implementation (bk-7); no tick (bk-4); no
  `atm-runtime` composition or CLI (bk-5).
