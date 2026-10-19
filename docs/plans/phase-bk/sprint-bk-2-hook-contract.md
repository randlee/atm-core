# Sprint bk-2 — core hook contract, sealed traits and protocol

closure_type `contract`; target_boundary the core hook contract
(`atm_core::hook`, the sealed `atm_core::boundary::{HookRunner,
CronScheduler, HookActionDispatcher}`), the six registry and schedule
protocol variants, and the `ATM_HOOK_*`/`ATM_CRON_*` error codes; branch
`sprint/bk-2-hook-contract`, PR → the stack top above bk-1
(`sprint/bk-1-hook-stores`); difficulty normal. Consumes bk-1: `HookName`,
`PhaseId`, `HookRegistration`, `HookRegistrationStore`, `CronSchedule`,
`NewCronSchedule`, `StopCondition`, `CronScheduleId`, `CronScheduleStore`,
`InMemoryHookRegistrationStore`, `InMemoryCronScheduleStore`. Governing
text: `design.md` §2, D-3, D-4, D-6, D-7, D-9; anchors ANC-003, ANC-007,
ANC-013, ANC-022, REQ-BK-D3, REQ-BK-ERRORS. Requirements
`REQ-CORE-HOOK-001`, `REQ-CORE-HOOK-002`, `REQ-P-HOOK-001` §23.2 (the
contract types); ADR-068 D1, D2, D3, D8, ADR-069 D2, D4 (`cron.stop` reply),
ADR-061 D5 (record of 2026-10-18 already on this branch).

## Scope boundary

This sprint owns every type and trait the runtime sprints consume and
nothing that runs a process, serves a request or touches SQLite: the hook
request/reply/action types, the three sealed boundary traits with their
`Noop` defaults and test doubles, the storage re-exports, the protocol
variants, the error codes and the OpenAPI schemas. It edits no file under
`crates/atm-storage*`, `crates/atm-http-runtime`,
`crates/atm-daemon-bootstrap`, `crates/atm` or `crates/atm-core/src/doctor`.

Owned paths (new unless marked edited):

- `crates/atm-core/src/hook/mod.rs`, `crates/atm-core/src/hook/request.rs`,
  `crates/atm-core/src/hook/reply.rs`,
  `crates/atm-core/src/boundary/hook_runner.rs`,
  `crates/atm-core/src/boundary/cron_scheduler.rs`,
  `crates/atm-core/src/boundary/hook_action_dispatcher.rs`,
  `crates/atm-core/src/boundary/mod.rs` (edited: re-exports below, the
  `mod.rs:10-36` pattern), `crates/atm-core/src/lib.rs` (edited:
  `pub mod hook`), `crates/atm-core/src/service_runtime.rs` (edited:
  `hook_registration_store()` and `cron_schedule_store()` next to
  `task_store()`, line 515), `crates/atm-core/src/protocol.rs` (edited: six
  request and six response variants, `HTTP_API_VERSION` to `1.13.0`),
  `crates/atm-core/src/api.rs` (edited: six `ApiRequest` variants mirroring
  the protocol ones, `From<RequestEnvelope>` arms at line 536,
  `ApiRequest::task_envelope`)
- `crates/atm-error/src/error_codes.rs` (edited: six variants),
  `docs/atm-error-codes.md` (edited: new `### 5.11 Task Hooks And Cron`
  with one `#### 5.11.n` entry per code and six `## 7.1` table rows)
- `docs/atm-http-runtime/openapi.yaml` and `crates/atm/openapi.yaml`
  (edited: `info.version` `1.13.0`, the twelve new schemas under
  `components.schemas`; the paths are bk-3's),
  `crates/atm/tests/openapi_surface_baseline.json` (edited: the version and
  schema additions), `docs/adr/ADR-061-governed-interface-schema-versioning.md`
  (edited: one D5 record line for `1.12.0` to `1.13.0`)
- `boundaries/atm-core/hook-runner.toml`,
  `boundaries/atm-core/cron-scheduler.toml`,
  `boundaries/atm-core/hook-action-dispatcher.toml` (the last two
  `visibility = "trait_only"`, format of
  `boundaries/atm-core/message-received-hook-emitter.toml`)
- `crates/atm-runtime-test-support/src/lib.rs` (edited: `pub use
  atm_storage::testing::{InMemoryHookRegistrationStore,
  InMemoryCronScheduleStore}`, `pub use atm_core::boundary::{DummyHookRunner,
  RecordingHookActionDispatcher, RecordingCronScheduler}`), so that
  `atm-http-runtime`, `atm-cron` and `atm` reach every double without an
  `atm_storage` import

## Public contract

```rust
// crates/atm-core/src/hook/request.rs  (design.md §2.1, ANC-003, ANC-022)
#[derive(Serialize, Deserialize)]
pub struct HookRequest {
    pub schema_version: String,   // "1.0.0"
    pub hook: HookName, pub team: TeamName, pub phase: PhaseId,
    pub caller: HookCaller,       // { identity: AgentName, team: TeamName, host: Option<HostName> }: host is the daemon's configured AtmConfig.local_host, null when the operator has not set it
    pub request: serde_json::Value, // the RequestEnvelope JSON (D-6); {"args": [...]} for cron.tick and cron.stop
    pub custom: serde_json::Value,  // passed through unchanged; atm defines nothing inside
}
pub const HOOK_SCHEMA_VERSION: &str = "1.0.0";   // independent of HTTP_API_VERSION (ADR-068 D8)
impl HookRequest { pub fn for_envelope(reg: &HookRegistration, caller: HookCaller, envelope: &RequestEnvelope) -> Result<Self, AtmError>;
                   pub fn for_cron(reg: &HookRegistration, caller: HookCaller, hook: HookName, args: &[String]) -> Self; }   // hook is cron.tick or cron.stop
pub fn hook_name_for(request: &ApiRequest) -> Option<HookName>;   // D-6 mapping on the router's type; None for un-hooked requests
// crates/atm-core/src/api.rs addition
impl ApiRequest { /// The six task-bearing variants back in wire form (Write => RequestEnvelope::Write, TaskMove => TaskMove, Messages with task_ledger Some => List); None otherwise.
    pub fn task_envelope(&self) -> Option<RequestEnvelope>; }

// crates/atm-core/src/hook/reply.rs  (design.md §2.2, ANC-004..007, ANC-009, D-9)
#[derive(Deserialize)]
#[serde(untagged)]
pub enum HookReply {
    Refused { success: False, message: String },
    Actions { success: True, #[serde(rename = "atm-action")] actions: Vec<HookAction> },
    PassThrough { success: True },
}
#[derive(Deserialize)]
pub struct HookAction { pub command: HookName, #[serde(rename = "as")] pub run_as: Option<AgentName>, pub request: serde_json::Value }
impl HookReply { pub fn parse(stdout: &[u8]) -> Result<Self, AtmError>; } // Err = AtmErrorCode::HookMalformed
/// Reply of `cron.stop`: exactly `{"stop": true, "reason": "<text>"}` or `{"stop": false}`; anything else is HookMalformed.
#[derive(Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StopReply { pub stop: bool, #[serde(default)] pub reason: Option<String> }
impl StopReply { pub fn parse(stdout: &[u8]) -> Result<Self, AtmError>; }  // stop == true without a reason, or stop == false with one, is HookMalformed
impl HookAction { pub fn envelope(&self) -> Result<RequestEnvelope, AtmError>; }
// envelope(): Err(HookMalformed) unless command.is_task_hook(); deserializes `request` as RequestEnvelope; Err(HookMalformed) unless
// hook_name_for(&ApiRequest::from(envelope.clone())) == Some(command) (D-6: an action can only be one of the six task commands, never a registry,
// cron or other route); then applies `as` to caller_identity on Write, TaskMove, List (ANC-007, ANC-009).

// crates/atm-core/src/boundary/hook_runner.rs  (design.md D-3, REQ-BK-D3)
pub struct HookOutcome { pub reply: HookReply, pub elapsed_ms: u64 }
#[async_trait]
pub trait HookRunner: sealed::Sealed + Send + Sync {
    /// Spawns the registered module once with `request` on stdin; Err is HookExit, HookTimeout or HookMalformed.
    async fn run(&self, registration: &HookRegistration, request: &HookRequest) -> Result<HookOutcome, AtmError>;
    /// `python3 <module> doctor`; same deadline rules. Used by `atm doctor` (ANC-015).
    async fn doctor(&self, registration: &HookRegistration) -> Result<serde_json::Value, AtmError>;
    /// `python3 <module> cron.stop` with `request` (hook cron.stop) on stdin; same deadline rules; parsed with StopReply::parse (ADR-069 D4).
    async fn stop(&self, registration: &HookRegistration, request: &HookRequest) -> Result<StopReply, AtmError>;
}
pub struct NoopHookRunner;                                           // production default when no runner is injected: Err(HookExit "no hook runner composed")
#[cfg(any(test, feature = "test-utils"))] pub struct DummyHookRunner { pub replies: Mutex<VecDeque<Result<HookOutcome, AtmError>>>, pub stop_replies: Mutex<VecDeque<Result<StopReply, AtmError>>>, pub calls: Mutex<Vec<(HookRegistration, HookRequest)>> }   // calls records run and stop requests alike

// crates/atm-core/src/boundary/cron_scheduler.rs  (design.md D-4; Rand 2026-10-18: atm cron in its own crate with a simple trait exposing its features)
#[async_trait]
pub trait CronScheduler: sealed::Sealed + Send + Sync {
    /// `cron::Schedule::from_str` on the 7-field seconds-first dialect; Err is AtmErrorCode::CronExprInvalid carrying the parser's message.
    fn validate_expression(&self, cron_expr: &str) -> Result<(), AtmError>;
    /// validate_expression, then CronScheduleStore::start, then the worker reloads (ADR-069 D2).
    fn start(&self, schedule: NewCronSchedule) -> Result<CronSchedule, AtmError>;
    /// CronScheduleStore::end (rows kept, active = false, ended_at set), then the worker reloads; returns the rows ended.
    fn end(&self, team: &TeamName, phase: &PhaseId, name: Option<&str>) -> Result<Vec<CronSchedule>, AtmError>;
    fn list(&self, team: Option<&TeamName>, phase: Option<&PhaseId>) -> Result<Vec<CronSchedule>, AtmError>;
    /// Runs one schedule now exactly as a tick does (stop check first); Ok(outcome string written by record_run or record_stop).
    async fn run_now(&self, id: CronScheduleId) -> Result<String, AtmError>;
}
pub struct NoopCronScheduler;                                        // router default until the bootstrap injects atm-cron's AtmCron: every method Err(AtmError::validation("no cron scheduler composed"))
#[cfg(any(test, feature = "test-utils"))] pub struct RecordingCronScheduler { pub rows: Mutex<Vec<CronSchedule>>, pub calls: Mutex<Vec<String>> }   // start/end/list over its own Vec, no parsing; calls records the method names

// crates/atm-core/src/boundary/hook_action_dispatcher.rs  (design.md §3 step 4, D-6; implemented by StorageAndNudgeRouter in bk-3, called by atm-cron in bk-4)
#[async_trait]
pub trait HookActionDispatcher: sealed::Sealed + Send + Sync {
    /// Dispatches `actions` in order with hooks disabled, as `default_as` unless an action names `as`, ingress AuthenticatedIngress::Local;
    /// stops at the first Err and returns it (ANC-006); Ok(n) is the number of actions that ran.
    async fn dispatch_actions(&self, hook: &HookName, team: &TeamName, default_as: &AgentName, actions: Vec<HookAction>) -> Result<usize, AtmError>;
}
pub struct NoopHookActionDispatcher;                                 // Err(validation("no hook action dispatcher composed"))
#[cfg(any(test, feature = "test-utils"))] pub struct RecordingHookActionDispatcher { pub calls: Mutex<Vec<(HookName, TeamName, AgentName, Vec<HookAction>)>>, pub fail_at: Option<usize> }   // Err(validation) when fail_at == Some(index of an action)

// crates/atm-core/src/boundary/mod.rs additions (the mod.rs:10-36 re-export pattern; atm-http-runtime, atm-cron and atm import only these)
pub use atm_storage::{HookRegistrationStore, HookRegistration, HookName, PhaseId, CronScheduleStore, CronSchedule, NewCronSchedule, CronScheduleId, StopCondition};
pub use cron_scheduler::{CronScheduler, NoopCronScheduler}; pub use hook_action_dispatcher::{HookActionDispatcher, NoopHookActionDispatcher}; pub use hook_runner::{HookRunner, HookOutcome, NoopHookRunner};
#[cfg(any(test, feature = "test-utils"))] pub use {cron_scheduler::RecordingCronScheduler, hook_action_dispatcher::RecordingHookActionDispatcher, hook_runner::DummyHookRunner};
```

```rust
// crates/atm-core/src/protocol.rs additions (HTTP API MINOR 1.12.0 -> 1.13.0)
RequestEnvelope::{ HookRegister(HookRegisterRequest), HookDisable(HookDisableRequest), HookList(HookListQuery),
                   CronStart(CronStartRequest), CronEnd(CronEndRequest), CronList(CronListQuery) }
ResponseEnvelope::{ HookRegister(HookRegistration), HookDisable(Vec<HookRegistration>), HookList(Vec<HookRegistration>),
                    CronStart(CronSchedule), CronEnd(Vec<CronSchedule>), CronList(Vec<CronSchedule>) }
pub struct HookRegisterRequest { pub caller_identity: AgentName, pub caller_team: TeamName, pub phase: PhaseId, pub hook: HookName, pub module_path: PathBuf, pub cwd: PathBuf, pub timeout_ms: u32 }
pub struct CronStartRequest { pub caller_identity: AgentName, pub caller_team: TeamName, pub phase: PhaseId, pub name: String, pub cron_expr: String, pub module_path: PathBuf, pub cwd: PathBuf, pub args: Vec<String>, pub timeout_ms: u32, pub stop: StopCondition }
// HookDisableRequest { caller_identity, caller_team, phase, hook: Option<HookName> }; HookListQuery { caller_team, phase: Option<PhaseId> }; CronEndRequest / CronListQuery alike.
// ApiRequest::{HookRegister, HookDisable, HookList, CronStart, CronEnd, CronList} carry the same payloads; From<RequestEnvelope> maps one to one.
// atm-error: AtmErrorCode::{HookMalformed => "ATM_HOOK_MALFORMED", HookExit => "ATM_HOOK_EXIT", HookTimeout => "ATM_HOOK_TIMEOUT",
//            HookModuleInvalid => "ATM_HOOK_MODULE_INVALID", HookActive => "ATM_HOOK_ACTIVE", CronExprInvalid => "ATM_CRON_EXPR_INVALID"}
// Display messages carry the design names: "HOOK.MALFORMED: ...", "HOOK.EXIT: ...", "HOOK.TIMEOUT: ...", "HOOK.ACTIVE: ...".
```

## Authoritative deliverables

1. `atm-core::hook`: `HookRequest`, `HookCaller`, `HOOK_SCHEMA_VERSION`,
   `for_envelope`, `for_cron`, `hook_name_for`, `HookReply`, `HookAction`,
   `StopReply`, `HookOutcome` as above. `HookRequest` is the one JSON
   request every hook receives, task hooks and cron hooks alike: the seven
   keys `schema_version`, `hook`, `team`, `phase`, `caller` (`identity`,
   `team`, `host` of the caller), `request`, `custom`, and nothing else;
   there is no second request shape (ANC-003). An action's `request` is
   the HTTP API body itself (the `RequestEnvelope` JSON the CLI would
   send); there is no separate action dialect, and `HookAction` has only
   `command`, `as` and `request` (ANC-007); `HookReply::parse` accepts exactly
   the three §2.2 shapes and rejects anything else with `HookMalformed`,
   running no subcommand logic of its own (the exit-code and deadline
   mapping to `HookExit` and `HookTimeout` is produced by bk-3's runner
   from the codes this sprint defines); `StopReply::parse` accepts exactly
   the two D-9 shapes; `HookAction::envelope` refuses a `command` outside
   the six task hooks and a `request` whose own `hook_name_for` is not
   `command` (both `HookMalformed`, D-6), then deserializes `request` with
   `RequestEnvelope` and overrides `caller_identity` with `as` on `Write`,
   `TaskMove`, `List`. `hook_name_for(&ApiRequest)` maps each of the six
   task-bearing requests to its own name (ANC-013), and
   `ApiRequest::task_envelope` gives the router the wire form to embed as
   `request` (D-6): `Write` with a task id and no `task_op` → `task.assign`,
   `Write` with `task_op` `Start` → `task.start`, `Close` → `task.close`,
   `TaskMove` → `task.move`, `List` with `task_ledger` `Tasks` →
   `task.list`, `Events` → `task.events`; every other envelope → `None`.
   Fail-closed by construction (REQ-BK-ERRORS): `HookReply::parse` and
   `StopReply::parse` return `Err(HookMalformed)` for any stdout that is
   not exactly one of their shapes, so a malformed reply can never be read
   as pass-through or as `stop == false`; `HookExit` and `HookTimeout` are
   errors the runner (bk-3) returns for a non-zero exit and a deadline, and
   no API maps any of the three to a success value.
2. `atm-core::boundary`: the three sealed traits `HookRunner`,
   `CronScheduler`, `HookActionDispatcher` with their `Noop` defaults and
   test doubles (test-utils), and the storage re-exports listed above, so
   that no crate but `atm-core` and `atm-cron` imports `atm_storage` for
   hook or cron types (`atm-http-runtime -> atm-storage` is a forbidden
   edge). One `HookRunner` instance is composed in the daemon and shared by
   the task router and the cron worker (bk-3, bk-4, bk-5); the `atm` crate
   depends on the trait's types only and never spawns a hook process
   (REQ-BK-D3). `LocalServiceRuntime` exposes `hook_registration_store()`
   and `cron_schedule_store()`.
3. Protocol: the six request and six response variants, the `ApiRequest`
   mirror in `api.rs:474` with `From<RequestEnvelope>`, `HTTP_API_VERSION`
   `"1.13.0"`. `CronStartRequest.stop` is a required `StopCondition`
   (never `Option`): a `CronStart` body without `stop`, or with a `kind`
   other than `until` or `hook`, fails to deserialize, so no schedule can
   be requested without exactly one stop condition (ANC-023, design D-9).
4. `atm-error`: the six `AtmErrorCode` variants with code strings and
   `from_str` arms; `docs/atm-error-codes.md` §5.11 entries (code,
   description, HTTP status `400 Bad Request` for `ATM_HOOK_MODULE_INVALID`,
   `ATM_HOOK_ACTIVE`, `ATM_CRON_EXPR_INVALID`; `500 Internal Server Error`
   for `ATM_HOOK_EXIT`, `ATM_HOOK_TIMEOUT`, `ATM_HOOK_MALFORMED`) and §7.1
   classification rows (`operator_actionable` for the first three,
   `retryable` for the last three); the ADR-061 D5 record line; both
   `openapi.yaml` copies and the surface baseline.
5. Boundary manifests for the three trait-only core boundaries:
   `hook-runner.toml` lists `allowed_test_double_paths =
   ["atm_core::boundary::hook_runner::DummyHookRunner"]`,
   `allowed_dependents = ["atm-http-runtime", "atm-daemon-bootstrap",
   "atm-cron", "atm"]`; `cron-scheduler.toml` (`allowed_dependents =
   ["atm-http-runtime", "atm-daemon-bootstrap", "atm-cron"]`, double
   `RecordingCronScheduler`); `hook-action-dispatcher.toml`
   (`allowed_dependents = ["atm-http-runtime", "atm-cron",
   "atm-daemon-bootstrap"]`, double `RecordingHookActionDispatcher`);
   `atm-runtime-test-support` re-exports the five doubles.
6. Tests, same unit, `atm-core` and `atm-error`:
   `hook_request_top_level_keys_and_request_shape_are_pinned` serializes
   `HookRequest` for all six task hooks, `cron.tick` and `cron.stop` and
   asserts the seven top-level keys and, per hook, that `request` equals
   the serde JSON of the envelope (`{"args": [...]}` for the cron hooks)
   (ANC-003, ADR-068 D8);
   `hook_action_envelope_refuses_non_task_command_and_mismatched_request`;
   `HookReply::parse` on the three shapes plus four malformed inputs;
   `StopReply::parse` on the two shapes plus `{"stop": true}`,
   `{"stop": false, "reason": "x"}` and `{"stop": "yes"}` (all
   `HookMalformed`); `HookAction::envelope` with and without `as` (ANC-007,
   ANC-009); `hook_name_for_maps_each_task_envelope`; protocol round trip
   of the twelve variants; the six error codes through `as_str`/`from_str`.

## Authoritative acceptance criteria

- [ ] (ANC-003) `boundary:HookRequest` one test serializes `HookRequest` for all six task hooks, `cron.tick` and `cron.stop` and asserts the identical top-level key set `schema_version, hook, team, phase, caller, request, custom` and that `caller` has exactly `identity`, `team`, `host`; no other request type exists in `atm_core::hook`.
- [ ] (ANC-022) `boundary:HookRequest` a `custom` value `{"x":[1,{"y":null}]}` round-trips byte-identical through `HookRequest`.
- [ ] (ANC-013) `boundary:HookRequest` `hook_name_for_maps_each_task_envelope` asserts the six envelope-to-name pairs above and `None` for `Peek`, `Search`, `Doctor`.
- [ ] (ANC-007, ANC-009) `boundary:HookReply` `HookAction::envelope` on a `task.close` action with `"as": "team-lead"` yields `RequestEnvelope::Write` with `caller_identity == team-lead`; without `as` the original `caller_identity` stands.
- [ ] (ANC-007) `boundary:HookReply` an action whose `request` is byte-for-byte the JSON body `atm task assign` sends over HTTP deserializes through `HookAction::envelope` into the same `RequestEnvelope` the HTTP handler would build; `HookAction` deserialization with `deny_unknown_fields` refuses any key other than `command`, `as`, `request`.
- [ ] (D-6) `boundary:HookReply` `HookAction::envelope` with `command = "cron.tick"`, with `command = "hook.register"`, and with `command = "task.close"` over a `task.assign` request each fail with `ATM_HOOK_MALFORMED`; a `request` that is a `HookRegister` or `CronStart` envelope fails the same way.
- [ ] (ANC-004, ANC-005, ANC-006) `boundary:HookReply` `HookReply::parse` maps `{"success":false,"message":"m"}` to `Refused`, `{"success":true}` to `PassThrough`, `{"success":true,"atm-action":[...]}` to `Actions` preserving order; `{"ok":true}`, empty stdout, two JSON objects and `{"success":"true"}` fail with `ATM_HOOK_MALFORMED`.
- [ ] (ANC-024) `boundary:HookReply` `StopReply::parse` accepts exactly `{"stop": true, "reason": "r"}` and `{"stop": false}`; `{"stop": true}`, `{"stop": false, "reason": "x"}`, `{"stop": "yes"}` and `{"stop": true, "reason": "r", "extra": 1}` fail with `ATM_HOOK_MALFORMED`.
- [ ] (REQ-BK-D3) `boundary:HookRunner` `HookRunner`, `CronScheduler` and `HookActionDispatcher` are sealed, their doubles are compiled only under `test-utils`, the three `atm-core` manifests list each as the sole test double; `just lint` boundary check passes with the three manifests.
- [ ] (REQ-BK-D3) `boundary:HookRunner` `rg -n 'atm_storage::' crates/atm-http-runtime/src crates/atm/src` matches nothing new: every hook and cron type reaches those crates through `atm_core::boundary` or `atm_runtime_test_support`; `cargo tree -p atm -e normal -i tokio` shows no new process-spawning dependency.
- [ ] (REQ-BK-ERRORS) `boundary:atm-error` the six codes round-trip through `AtmErrorCode::as_str`/`from_str`, and `docs/atm-error-codes.md` lists each of the six in §5.11 and §7.1 (`grep -c` equals 6 for each section).
- [ ] (REQ-BK-ERRORS) `boundary:HookReply` a table of ten malformed stdouts (empty, whitespace, `null`, `[]`, `{"success":"true"}`, `{"success":true,"atm-action":{}}`, `{"success":false}`, two concatenated objects, truncated JSON, `{"stop":1}`) yields `Err` with code `ATM_HOOK_MALFORMED` from `HookReply::parse` and from `StopReply::parse`; neither function has a path returning `PassThrough` or `stop == false` from invalid input.
- [ ] (ANC-023) `boundary:atm-protocol` `cargo test -p atm --test openapi_surface` passes with `info.version == "1.13.0"` in both `openapi.yaml` copies and the twelve request/response schemas present; `CronStartRequest.stop` is required with `kind` one of `until`, `hook`; a `CronStart` JSON body without `stop` fails to deserialize into `RequestEnvelope`, and one with `{"kind":"until","at":"2026-11-01T00:00:00Z"}` round-trips to `StopCondition::Until`.
- [ ] `boundary:atm-protocol` the twelve new envelope variants serialize and deserialize; `HTTP_API_VERSION == "1.13.0"`; `From<RequestEnvelope>` maps each new variant to its `ApiRequest` twin.

## Authoritative validation

- `cargo build --workspace`
- `cargo test -p atm-core --features test-utils -p atm-error -p atm --test openapi_surface`
- `cargo clippy -p atm-core -p atm-error -p atm-runtime-test-support --all-targets -- -D warnings`
- `just lint` (boundary manifests, `.just/check_line_counts.py`, RULE-003)

## Non-closure

- No hook process is spawned (bk-3 `DaemonHookRunner`); no router consults
  a hook (bk-3); no tick or stop check fires (bk-4); no CLI command exists
  (bk-5); no `TIMEOUT_MS` is read from a module (bk-5); no doctor section
  (bk-5); no HTTP path serves the six new variants (bk-3 adds the routes
  and their `openapi.yaml` paths); user documentation (bk-5). ADR-068,
  ADR-069 and the ADR-061 D6 record landed with the plan PR #1877.
- `NoopHookRunner`, `NoopCronScheduler` and `NoopHookActionDispatcher` are
  the composed defaults until bk-3 and bk-5 inject the real ones; with
  them every hook consult would fail closed, which no route reaches in
  this sprint.
- The two store tables and their rusqlite code are bk-1's; this sprint
  only re-exports bk-1's types.
