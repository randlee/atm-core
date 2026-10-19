# Sprint bk-2 — core hook contract, sealed traits and protocol

closure_type `contract`; target_boundary the core hook contract
(`atm_core::hook`, the sealed `atm_core::boundary::{TaskCommands,
HookRunner, CronScheduler, HookActionDispatcher}`), the six registry and
schedule protocol variants and their routes, and the store wiring into
`LocalServiceRuntime`; branch `sprint/bk-2-hook-contract`, PR → the stack
top above bk-1 (`sprint/bk-1-hook-stores`); difficulty normal;
recommended_agent `arch-ctm`, recommended_model `gpt-5-codex`. Consumes
bk-1: `HookName`, `PhaseId`, `HookRegistration`, `HookRegistrationStore`,
`CronSchedule`, `NewCronSchedule`, `StopCondition`, `CronScheduleId`,
`CronScheduleStore`, the two in-memory doubles and the six error codes
and constructors. Governing text: `design.md` §2, §9.2, §9.3, §9.4, §9.5,
§9.11, D-3, D-4, D-6, D-7, D-9, D-10; anchors ANC-003, ANC-007, ANC-013,
ANC-022, REQ-BK-D3, REQ-BK-ERRORS. Requirements `REQ-CORE-HOOK-001`,
`REQ-P-HOOK-001` §23.2; ADR-068 D1, D2, D3, D8, ADR-069 D2, D4, ADR-061 D5.

## Scope boundary

This sprint owns every type, trait and route the runtime sprints consume
and nothing that runs a process, serves a request or touches SQLite. It
edits no file under `crates/atm-storage*`, `crates/atm-error`,
`crates/atm-http-runtime`, `crates/atm-daemon-bootstrap`, `crates/atm/src`
or `crates/atm-cron`.

Owned paths (new unless marked edited):

- `crates/atm-core/src/hook/mod.rs`, `hook/request.rs`, `hook/reply.rs`
  (under `crates/atm-core/src/`), `crates/atm-core/src/boundary/task_commands.rs`,
  `boundary/hook_runner.rs`, `boundary/cron_scheduler.rs`,
  `boundary/hook_action_dispatcher.rs`, `crates/atm-core/src/boundary/mod.rs`
  (edited: four `mod` lines after `mod template_composer;` at `:86` and the
  re-exports below), `crates/atm-core/src/lib.rs` (edited: `pub mod hook`)
- `crates/atm-core/src/protocol.rs` (edited: six request and seven
  response variants, `HTTP_API_VERSION` `1.13.0`),
  `crates/atm-core/src/api.rs` (edited: the six routes of `design.md`
  §9.11, `HOOK_ACTIONS_HEADER = "X-ATM-Hook-Actions"` next to
  `CLEAR_OUTCOME_HEADER` (`:31`), the `ApiRequest` variants),
  `crates/atm-core/src/transport/testing.rs` (edited: the new variants in
  `execute_inner`, about `:110-170`)
- `crates/atm-core/src/service_runtime.rs` (edited: two fields, builders
  `with_hook_registration_store`/`with_cron_schedule_store` and accessors
  next to `task_store` (`:325`, `:506`, `:515`)),
  `crates/atm-runtime/src/composition.rs` (edited: the two stores next to
  `storage.task_store()` `:178` and `.with_task_store` `:193`);
  `crates/atm/src/composition.rs` installs no store (no `with_task_store`
  call there) and is unchanged
- `crates/atm-core/src/doctor/hooks.rs` (new: `HookDoctorEntry`,
  `HookDoctorError`), `crates/atm-core/src/doctor/mod.rs` (edited: `pub mod
  hooks`), `crates/atm-core/src/doctor/report.rs` (edited: `hooks` on
  `DoctorReport`, `:467`)
- `docs/atm-http-runtime/openapi.yaml`, `crates/atm/openapi.yaml`
  (edited: `info.version` `1.13.0`, the six paths, the new request,
  response and doctor schemas), `crates/atm/tests/openapi_surface_baseline.json`,
  `docs/adr/ADR-061-governed-interface-schema-versioning.md` (edited: one
  D5 record line for `1.12.0` to `1.13.0`)
- `boundaries/atm-core/task-commands.toml` (`BOUNDARY-TaskCommands`), `boundaries/atm-core/hook-runner.toml` (`BOUNDARY-HookRunner`),
  `boundaries/atm-core/cron-scheduler.toml` (`boundary_id =
  "BOUNDARY-CronSchedulerTrait"`, distinct from bk-4's
  `BOUNDARY-CronScheduler`), `boundaries/atm-core/hook-action-dispatcher.toml` (`BOUNDARY-HookActionDispatcher`)
  (all `visibility = "trait_only"`, `constructor = "none"`, format of
  `boundaries/atm-core/message-received-hook-emitter.toml`, implementers
  named in `[status] notes`)
- `crates/atm-runtime-test-support/src/lib.rs` (edited: `pub use
  atm_storage::testing::{InMemoryHookRegistrationStore,
  InMemoryCronScheduleStore}` and `pub use atm_core::boundary::{DummyHookRunner,
  RecordingHookActionDispatcher, RecordingCronScheduler,
  RecordingTaskCommands}`)

## Public contract

```rust
// crates/atm-core/src/hook/request.rs  (design.md §2.1, ANC-003, ANC-022)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HookCaller { pub identity: AgentName, pub team: TeamName, pub host: Option<HostName> }   // types via atm_core::types
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookRequest {
    pub schema_version: String,     // HOOK_SCHEMA_VERSION
    pub hook: HookName, pub team: TeamName, pub phase: PhaseId, pub caller: HookCaller,
    pub request: serde_json::Value, // the bare HTTP body (TaskCommand::request_body); {"args": [...], "created_by": "<agent>"} for cron.tick and cron.stop
    pub custom: serde_json::Map<String, serde_json::Value>,  // the registration's (or schedule's) stored custom, unchanged (Rand 2026-10-18)
}
pub const HOOK_SCHEMA_VERSION: &str = "1.0.0";   // independent of HTTP_API_VERSION (ADR-068 D8)
pub const HOOK_CUSTOM_MAX_BYTES: usize = 65_536;   // serialized size cap on a registration's custom (design.md §2.1)
/// The route's check (bk-3): a non-object or a serialized size above HOOK_CUSTOM_MAX_BYTES is hook_module_invalid naming the limit.
pub fn custom_object(value: serde_json::Value) -> Result<serde_json::Map<String, serde_json::Value>, AtmError>;
impl HookRequest {
    pub fn for_task(reg: &HookRegistration, caller: HookCaller, command: &TaskCommand) -> Self;   // custom = reg.custom
    pub fn for_cron(reg: &HookRegistration, caller: HookCaller, hook: HookName, args: &[String]) -> Self;   // hook is cron.tick or cron.stop; custom = reg.custom; request.created_by = caller.identity
}

// crates/atm-core/src/hook/reply.rs  (design.md §9.2, §9.4)
pub enum HookReply { Handled(Vec<HookAction>), PassThrough, NotApplicable, Refused { message: String } }   // no serde derive
impl HookReply { pub fn parse(stdout: &[u8]) -> Result<Self, AtmError>; }                  // §9.4; every other stdout is AtmError::hook_malformed
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookAction { pub command: String, #[serde(rename = "as", default)] pub run_as: Option<AgentName>, pub request: serde_json::Value }
impl HookAction { pub fn into_api_request(&self) -> Result<ApiRequest, AtmError>; }       // §9.2 table; `as` replaces caller_identity on Write, TaskMove, List
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopReply { pub stop: bool, #[serde(default)] pub reason: Option<String> }
impl StopReply { pub fn parse(stdout: &[u8]) -> Result<Self, AtmError>; }                 // {"stop":true,"reason":"<non-empty>"} | {"stop":false}

// crates/atm-core/src/boundary/task_commands.rs: the trait, TaskCallCtx, the five newtypes and TaskCommand exactly as design.md §9.5
#[cfg(any(test, feature = "test-utils"))]
pub struct RecordingTaskCommands { pub calls: Mutex<Vec<(HookName, serde_json::Value)>>, pub response: Mutex<Option<ApiResponse>> }

// crates/atm-core/src/boundary/hook_runner.rs  (design.md D-3, §9.7)
#[derive(Debug)] pub struct HookOutcome { pub reply: HookReply, pub elapsed_ms: u64 }
#[async_trait]
pub trait HookRunner: sealed::Sealed + Send + Sync {
    /// Spawns the module once with `request` on stdin under `deadline` (§9.7); Err is HookExit, HookTimeout or HookMalformed.
    async fn run(&self, registration: &HookRegistration, request: &HookRequest, deadline: RequestDeadline) -> Result<HookOutcome, AtmError>;
    /// argv "doctor", stdin HookDoctorRequest::for_registration(registration); Ok is the parsed HookDoctorReply (design.md §9.10).
    async fn doctor(&self, registration: &HookRegistration, deadline: RequestDeadline) -> Result<HookDoctorReply, AtmError>;
    async fn stop(&self, registration: &HookRegistration, request: &HookRequest, deadline: RequestDeadline) -> Result<StopReply, AtmError>;
}
#[cfg(any(test, feature = "test-utils"))]
pub struct DummyHookRunner { pub replies: Mutex<VecDeque<Result<HookOutcome, AtmError>>>, pub stop_replies: Mutex<VecDeque<Result<StopReply, AtmError>>>,
                             pub doctor_replies: Mutex<VecDeque<Result<HookDoctorReply, AtmError>>>, pub calls: Mutex<Vec<(HookRegistration, HookRequest)>>, pub panic_on_run: AtomicBool }

// crates/atm-core/src/boundary/cron_scheduler.rs  (design.md D-4: start, end, list only)
#[async_trait]
pub trait CronScheduler: sealed::Sealed + Send + Sync {
    async fn start(&self, schedule: NewCronSchedule) -> Result<CronSchedule, AtmError>;
    async fn end(&self, team: &TeamName, phase: &PhaseId, name: Option<&str>) -> Result<Vec<CronSchedule>, AtmError>;
    async fn list(&self, team: Option<&TeamName>, phase: Option<&PhaseId>) -> Result<Vec<CronSchedule>, AtmError>;
}
#[cfg(any(test, feature = "test-utils"))] pub struct RecordingCronScheduler { pub rows: Mutex<Vec<CronSchedule>>, pub calls: Mutex<Vec<&'static str>> }

// crates/atm-core/src/boundary/hook_action_dispatcher.rs  (implemented by the base router in bk-3, called by bk-3's filter and by atm-cron)
#[async_trait]
pub trait HookActionDispatcher: sealed::Sealed + Send + Sync {
    /// Checks every action first (design §2.2: caller_team == team, `as` on the roster, into_api_request Ok), else HookMalformed and nothing runs.
    /// Then runs them in order un-hooked as `default_as` unless `as`; `deadline` Some is reused, None gives each action now + SERVER_REQUEST_BUDGET.
    /// Stops at the first Err with the design §2.2 prefix; Ok holds every response in order.
    async fn dispatch_actions(&self, hook: &HookName, team: &TeamName, phase: &PhaseId, default_as: &AgentName,
                              actions: Vec<HookAction>, deadline: Option<RequestDeadline>) -> Result<Vec<(String, ApiResponse)>, AtmError>;
}
#[cfg(any(test, feature = "test-utils"))] pub struct RecordingHookActionDispatcher { pub calls: Mutex<Vec<(HookName, TeamName, AgentName, Vec<HookAction>)>>, pub fail_at: Option<usize> }

// crates/atm-core/src/boundary/mod.rs: mod task_commands; mod hook_runner; mod cron_scheduler; mod hook_action_dispatcher; then
pub use atm_storage::{HookRegistrationStore, HookRegistration, HookName, PhaseId, CronScheduleStore, CronSchedule, NewCronSchedule, CronScheduleId, StopCondition};
pub use atm_storage::request_budget::{HOOK_TIMEOUT_MAX_MS, CRON_TIMEOUT_MAX_MS, HOOK_ACTION_RESERVE_MS, SERVER_REQUEST_BUDGET};
pub use task_commands::*; pub use cron_scheduler::CronScheduler; pub use hook_action_dispatcher::HookActionDispatcher; pub use hook_runner::{HookRunner, HookOutcome};
#[cfg(any(test, feature = "test-utils"))] pub use {cron_scheduler::RecordingCronScheduler, hook_action_dispatcher::RecordingHookActionDispatcher, hook_runner::DummyHookRunner};

// crates/atm-core/src/protocol.rs (HTTP API MINOR 1.12.0 -> 1.13.0); request fields are design.md §9.11
RequestEnvelope::{ HookRegister(HookRegisterRequest), HookDisable(HookDisableRequest), HookList(HookListQuery),
                   CronStart(CronStartRequest), CronEnd(CronEndRequest), CronList(CronListQuery) }
ResponseEnvelope::{ HookRegister(HookRegistration), HookDisable(Vec<HookRegistration>), HookList(Vec<HookRegistration>),
                    CronStart(CronSchedule), CronEnd(Vec<CronSchedule>), CronList(Vec<CronSchedule>), HookActions(HookActionsResponse) }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] pub struct HookActionsResponse { pub dispatched: usize }   // design D-b

// crates/atm-core/src/doctor/hooks.rs  (design.md §9.10)
// Rand 2026-10-18: "doctor would need to know team, phase and it would return a dict/array (whatever is consistent w/ what doctor expects)"
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] #[serde(deny_unknown_fields)]
pub struct HookDoctorRequest { pub schema_version: String, pub hook: String /* always "doctor" */, pub team: TeamName, pub phase: PhaseId, pub custom: serde_json::Map<String, serde_json::Value> }
impl HookDoctorRequest { pub fn for_registration(reg: &HookRegistration) -> Self; }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] #[serde(deny_unknown_fields)]
pub struct HookDoctorReply { pub success: bool, pub message: String, #[serde(default)] pub findings: Vec<HookDoctorFinding> }
impl HookDoctorReply { pub fn parse(stdout: &[u8]) -> Result<Self, AtmError>; }   // any other shape: hook_malformed
/// The fields of DoctorFinding (report.rs:38); `code` is the module's own string, not an AtmErrorCode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] #[serde(deny_unknown_fields)]
pub struct HookDoctorFinding { pub severity: DoctorSeverity, pub code: String, pub message: String, #[serde(default)] pub remediation: Option<String> }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDoctorEntry { pub module_path: PathBuf, pub cwd: PathBuf, pub team: TeamName, pub phase: PhaseId, pub hooks: Vec<HookName>, pub schedules: Vec<String>, pub timeout_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub message: Option<String>,
    #[serde(default)] pub findings: Vec<HookDoctorFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub error: Option<HookDoctorError> }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] pub struct HookDoctorError { pub code: AtmErrorCode, pub message: String }
// report.rs: DoctorReport gains `#[serde(default)] pub hooks: Vec<HookDoctorEntry>` (no skip_serializing_if)

// crates/atm-core/src/service_runtime.rs, pattern task_store :506/:515
pub fn with_hook_registration_store(self, store: Arc<dyn HookRegistrationStore>) -> Self;
pub fn hook_registration_store(&self) -> Result<Arc<dyn HookRegistrationStore>, AtmError>;   // not installed: the task_store error shape naming the store
pub fn with_cron_schedule_store(self, store: Arc<dyn CronScheduleStore>) -> Self;
pub fn cron_schedule_store(&self) -> Result<Arc<dyn CronScheduleStore>, AtmError>;
```

## Authoritative deliverables

1. `atm-core::hook`: the request, reply, action and stop types above,
   parsed by the rules of `design.md` §9.4 and mapped by §9.2. A reply
   that is not exactly one of the four shapes is `ATM_HOOK_MALFORMED`
   returned to the caller, never pass-through or not-applicable and never the normal
   command (Rand, 2026-10-18: "manformed hook MUST return hook failed to
   caller (error)"). `HookRequest` is the one request every hook receives,
   seven keys and nothing else (ANC-003); an action's `request` is the
   bare HTTP body itself and `HookAction` has only `command`, `as` and
   `request` (ANC-007).
2. `atm-core::boundary`: the four sealed traits with their doubles behind
   `test-utils`, `TaskCallCtx`, the five newtypes, `TaskCommand` with
   `classify` by the `design.md` §9.3 table, `hook_name` and
   `request_body`, and the re-exports, so that no crate but `atm-core`
   imports `atm_storage` for hook or cron types. There are no `Noop`
   types.
3. Protocol and routes: the variants above, `HTTP_API_VERSION` `"1.13.0"`,
   the six routes and every arm `design.md` §9.11 assigns to bk-2,
   `HookActions` decoded from `HOOK_ACTIONS_HEADER` before
   `decode_success_response`. `CronStartRequest.stop` is a required
   `StopCondition`. `HookRegisterRequest.custom` and
   `CronStartRequest.custom` are `#[serde(default)]` JSON (absent means
   `{}`); the route turns them into the stored map with `custom_object`.
   `CronStartRequest.timeout_ms` is capped by `CRON_TIMEOUT_MAX_MS`, not
   `HOOK_TIMEOUT_MAX_MS`, in both `openapi.yaml` copies.
4. Store wiring: the `LocalServiceRuntime` builders and accessors and the
   `atm-runtime` composition; both store manifests from bk-1 gain
   `atm-runtime` in `allowed_dependents` only if bk-1 did not list it (it
   does).
5. Doctor request, reply and report types and both `openapi.yaml` copies
   (the `HookDoctorEntry` and `HookDoctorFinding` schemas under
   `DoctorReport.hooks`) with the surface baseline and the ADR-061 D5 record line.
6. Boundary manifests: `allowed_test_double_paths` use the `crate::`
   form (`["crate::boundary::hook_runner::DummyHookRunner"]`, and
   likewise for the other three); `allowed_dependents` list
   `atm-http-runtime`, `atm-daemon-bootstrap` and, for `hook-runner.toml`,
   `atm`; `atm-cron` is added by bk-4 when that edge exists.
7. Tests, same unit, in the files the criteria name.

## Authoritative acceptance criteria

- [ ] (ANC-003) `boundary:BOUNDARY-HookRunner` `crates/atm-core/src/hook/request.rs::tests::hook_request_top_level_keys_and_request_shape_are_pinned` serializes `HookRequest` for all six task commands, `cron.tick` and `cron.stop` and asserts the key set `schema_version, hook, team, phase, caller, request, custom`, `caller` keys `identity, team, host`, and per task command `request == serde_json::from_slice(encode_request_body(..))` for the envelope the CLI sends; `{"args": [...], "created_by": <caller.identity>}` for the cron hooks.
- [ ] (ANC-022) `boundary:BOUNDARY-HookRunner` `request.rs::tests::custom_round_trips_and_comes_from_the_registration`: `custom` `{"x":[1,{"y":null}]}` round-trips byte-identical; `for_task` and `for_cron` copy `reg.custom` unchanged and never add or drop a key; `for_cron` sets `request.created_by` to `caller.identity`; `::custom_object_refuses_non_object_and_oversize`: `[1]`, `"s"` and an object serializing to 65 537 bytes are `ATM_HOOK_MODULE_INVALID` naming `custom`, and one of 65 536 bytes is accepted.
- [ ] (ANC-013) `boundary:BOUNDARY-TaskCommands` `crates/atm-core/src/boundary/task_commands.rs::tests::classify_maps_each_task_request` asserts every row of `design.md` §9.3, including legacy `task_complete` → `close`, `acknowledges_message_id` → not a task command, `History` → not a task command, and `hook_name()` of each variant equal to its method's hook name (`Assign` → `task.assign` ... `Events` → `task.events`), and `::normalization_error_surfaces_the_existing_validation_error` (a `Write` whose `task_op_normalized` fails is returned unclassified).
- [ ] (ANC-007, ANC-009) `boundary:BOUNDARY-HookRunner` `crates/atm-core/src/hook/reply.rs::tests::action_maps_each_command`: one action per row of `design.md` §9.2 yields the named `ApiRequest`; `as: team-lead` sets `caller_identity` on Write, TaskMove and List; without `as` the body's own value stands; a `request` byte-identical to the body `atm task assign` sends gives the request the HTTP handler builds.
- [ ] (D-6) `boundary:BOUNDARY-HookRunner` `reply.rs::tests::registry_cron_and_internal_commands_are_malformed`: `hook.register`, `cron.start`, `heartbeat`, `task.foo`, and `send` with a `task_id` each fail with `ATM_HOOK_MALFORMED`; a key other than `command`, `as`, `request` is refused by `deny_unknown_fields`.
- [ ] (ANC-004, ANC-005, ANC-006, REQ-BK-ERRORS) `boundary:BOUNDARY-HookRunner` `reply.rs::tests::reply_parse_case_table` runs every row of `design.md` §9.4: the four valid shapes map to `Handled` (order kept), `PassThrough`, `NotApplicable` and `Refused`, and every listed malformed stdout is `Err` with code `ATM_HOOK_MALFORMED`; no path returns `PassThrough` or `NotApplicable` for invalid input; `unknown_result_value_is_malformed` (`{"result":"skip"}`, `{"result":true}`); `success_key_from_old_schema_is_malformed` (`{"success":true}`, `{"success":false,"message":"m"}`).
- [ ] (ANC-024) `boundary:BOUNDARY-HookRunner` `reply.rs::tests::stop_reply_shapes`: accepts exactly `{"stop": true, "reason": "r"}` and `{"stop": false}`; `{"stop": true}`, `{"stop": true, "reason": ""}`, `{"stop": false, "reason": "x"}`, `{"stop": "yes"}`, `{"stop":1}` and an extra key fail with `ATM_HOOK_MALFORMED`.
- [ ] (REQ-BK-ERRORS) `boundary:BOUNDARY-HookRunner` `reply.rs::tests::malformed_is_never_pass_through`: for each malformed row of §9.4 `HookReply::parse` returns `Err` with code `ATM_HOOK_MALFORMED` and never `Ok(PassThrough)` or `Ok(NotApplicable)`; `HookRunner::run`'s doc states its `Err` set is `ATM_HOOK_EXIT`, `ATM_HOOK_TIMEOUT`, `ATM_HOOK_MALFORMED`.
- [ ] (REQ-BK-D3) `boundary:BOUNDARY-HookRunner` `HookRunner` is the one runner trait: `AtmCronDeps.runner` (bk-4) and `HookedTaskCommands.runner` (bk-3) both take `Arc<dyn HookRunner>`, so one daemon instance serves both; the four traits are sealed with `atm_core::boundary::sealed::Sealed`, their doubles compile only under `test-utils`, each manifest lists its double as the sole test double; `just lint` boundary check passes with the four manifests; `rg -n 'atm_storage::' crates/atm-http-runtime/src crates/atm/src` matches nothing new.
- [ ] `boundary:BOUNDARY-AtmProtocol` `crates/atm-core/src/api.rs::tests::every_new_route_round_trips_encode_http_request_to_http_route_kind`: each of the six new envelopes encodes to `POST` on its §9.11 path and decodes back to its `HttpRouteKind`; `::hook_actions_header_decodes_to_hook_actions` turns a 200 with `X-ATM-Hook-Actions: 2` into `ResponseEnvelope::HookActions(HookActionsResponse { dispatched: 2 })`.
- [ ] (ANC-023) `boundary:BOUNDARY-AtmProtocol` `cargo test -p atm --test openapi_surface` passes with `info.version == "1.13.0"` in both copies, the six paths and the new schemas; a `CronStart` body without `stop` fails to deserialize and `{"kind":"until","at":"2026-11-01T00:00:00Z"}` round-trips to `StopCondition::Until`.
- [ ] `boundary:BOUNDARY-AtmRuntime-Composition` `crates/atm-runtime/src/composition.rs::tests::composition_installs_the_hook_and_cron_stores` (pattern `:347`) and `crates/atm-core/src/service_runtime.rs::tests::hook_and_cron_stores_report_the_not_installed_error` (pattern `:1264`).
- [ ] (REQ-P-HOOK-004) `boundary:BOUNDARY-DaemonRuntimeDoctorReport` `crates/atm-core/src/doctor/hooks.rs::tests::report_without_hooks_field_decodes_and_empty_hooks_serialize`: a pre-bk JSON report decodes with `hooks == []`, and a report with no entries serializes `"hooks": []`.
- [ ] (ANC-015, Rand 2026-10-18 doctor) `boundary:BOUNDARY-DaemonRuntimeDoctorReport` `doctor/hooks.rs::tests::doctor_request_and_reply_shapes`: `HookDoctorRequest::for_registration` serializes exactly `schema_version, hook, team, phase, custom` with `hook == "doctor"` and the row's `custom`; `HookDoctorReply::parse` accepts `{"success":true,"message":"ok"}` (`findings == []`) and the design.md §9.10 example; `{"success":true}`, `{"message":"x"}`, an extra key, `"severity":"fatal"`, a finding without `code`, `findings: {}` and `ok` are each `ATM_HOOK_MALFORMED`; `HookDoctorFinding` has the four field names of `DoctorFinding`.
- [ ] `boundary:BOUNDARY-AtmProtocol` the thirteen new envelope variants round-trip through serde; `HTTP_API_VERSION == "1.13.0"`; `From<RequestEnvelope>` and `into_inner` map each new variant both ways.

## Authoritative validation

- `cargo build --workspace`
- `cargo test -p atm-core --features test-utils -p atm-runtime -p atm --test openapi_surface`
- `cargo clippy -p atm-core -p atm-runtime -p atm-runtime-test-support --all-targets -- -D warnings`
- `just lint` (boundary manifests, `.just/check_line_counts.py`, RULE-003)

## Non-closure

- No hook process is spawned (bk-3 `DaemonHookRunner`); no filter or
  normal implementation of `TaskCommands` exists (bk-3); no tick or stop
  check fires (bk-4); no CLI command, `TIMEOUT_MS` read, exit-code arm or
  doctor collection (bk-5). ADR-068, ADR-069 and the ADR-061 D6 record
  landed with the plan PR #1877.
- Error codes and constructors are bk-1's; this sprint only uses them.
