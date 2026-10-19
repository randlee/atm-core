# Sprint bk-1 — hook registration and cron schedule store contracts

closure_type `contract`; target_boundary `BOUNDARY-HookRegistrationStore`
and `BOUNDARY-CronScheduleStore` (atm-storage, new, sealed) with their
in-memory doubles, and the six hook and cron error codes; branch
`sprint/bk-1-hook-stores`, PR → `integrate/phase-bk` (layer 1); difficulty
normal; recommended_agent `arch-ctm`, recommended_model `gpt-5-codex`
(normal tier). The rusqlite implementations are bk-6. Governing text:
`design.md` §4, §9.1 (inlined below), §9.7 constants, D-5, D-7, D-8, D-9,
D-i; anchors ANC-002, ANC-018, ANC-020, ANC-023, REQ-BK-ERRORS.
Requirements `REQ-P-HOOK-001` §23.2, `REQ-P-CRON-001` §24.1; ADR-068 D1,
D5, ADR-069 D1, D4, ADR-063 D3 as amended by ADR-069 D5 (fourteen
capabilities; Rand round 5: "yes, we need storage for cron"). Rand
(2026-10-18): "error codes need to move to bk-1".

## Scope boundary

This sprint owns the two storage capabilities every later sprint reads or
writes through and the error codes they raise: the store traits, their
row and id types, the in-memory doubles, the six `AtmErrorCode` variants
with mappings and docs, the `AtmError` constructors with `HookExitCause`,
and the three budget constants. It edits no file under
`crates/atm-storage-rusqlite` (bk-6), `crates/atm-core` (bk-2),
`crates/atm-http-runtime`, `crates/atm-daemon-bootstrap` or `crates/atm`.

Owned paths (new unless marked edited):

- `crates/atm-storage/src/hook_registration_store.rs`,
  `crates/atm-storage/src/cron_schedule_store.rs`,
  `crates/atm-storage/src/hook_error.rs` (`HookExitCause` and the six
  constructors as an `impl AtmError` block),
  `crates/atm-storage/src/lib.rs` (edited: three `pub mod` + re-exports),
  `crates/atm-storage/src/testing/hook_registration_store.rs`,
  `crates/atm-storage/src/testing/cron_schedule_store.rs` (the doubles),
  `crates/atm-storage/src/testing.rs` (edited: two `mod` lines and two
  `pub use` lines only; it has 909 production lines, so it gains 4 and
  stays under the 1000-line cap),
  `crates/atm-storage/src/error.rs` (edited: three `is_validation` arms,
  `error.rs:102`; 462 production lines),
  `crates/atm-storage/src/error_catalog.rs` (edited: guidance arms, empty
  for `HookRefused`; 305 lines), `crates/atm-storage/src/request_budget.rs`
  (edited: the three constants after `SERVER_REQUEST_BUDGET`, `:44`, and a
  `const _` assertion), `crates/atm-storage/tests/hook_stores_contract.rs`
- `crates/atm-error/src/error_codes.rs` (edited: six variants, code
  strings, `from_str` arms; 552 lines), `crates/atm-error/src/lib.rs`
  (edited: the six codes in the wire round-trip tables, `:55-110`),
  `docs/atm-error-codes.md` (edited: `### 5.11 Task Hooks And Cron`, one
  `#### 5.11.n` per code, six rows in `### 7.1 Classification By Code`)
- `boundaries/atm-storage/hook-registration-store.toml`,
  `boundaries/atm-storage/cron-schedule-store.toml` (format of
  `task-store.toml`)
- `docs/atm-storage/boundaries.md` (edited: `## HookRegistrationStore`
  and `## CronScheduleStore` after `## TaskStore`),
  `docs/adr/ADR-036-storage-boundary-and-composition-topology.md` (one
  amendment paragraph, format of its Phase AQ amendment, naming the two
  capabilities under ADR-069 D5)

Line counts are `.just/check_line_counts.py` production lines measured on
`develop` at the plan head; every new type lives in a new file.

## Public contract

```rust
// crates/atm-storage/src/hook_registration_store.rs
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HookName(String);   // task.assign|task.start|task.close|task.move|task.list|task.events|cron.tick|cron.stop
impl HookName {
    pub const TASK_HOOKS: [&'static str; 6] = ["task.assign", "task.start", "task.close", "task.move", "task.list", "task.events"];
    pub const CRON_TICK: &'static str = "cron.tick";
    pub const CRON_STOP: &'static str = "cron.stop";
    pub fn parse(name: &str) -> Result<Self, AtmError>;            // any of the eight, else AtmError::hook_malformed("unknown hook name <n>")
    pub fn parse_task_hook(name: &str) -> Result<Self, AtmError>;  // exactly TASK_HOOKS, else hook_module_invalid (the register route's only validator, ADR-068 D3)
    pub fn as_str(&self) -> &str;
}
impl TryFrom<String> for HookName { type Error = AtmError; }  impl From<HookName> for String {}  impl std::fmt::Display for HookName {}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PhaseId(String);    // ^[a-z][a-z0-9-]{0,31}$, std char tests, no regex crate
impl PhaseId { pub fn parse(s: &str) -> Result<Self, AtmError>; pub fn as_str(&self) -> &str; }
// Err: AtmError::validation_with_recovery (ATM_MESSAGE_VALIDATION_FAILED) "phase id <s> must match ^[a-z][a-z0-9-]{0,31}$",
// recovery "use a lowercase phase id such as bk or bk-2"
impl TryFrom<String> for PhaseId { type Error = AtmError; }  impl From<PhaseId> for String {}  impl std::fmt::Display for PhaseId {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRegistration {
    pub team: TeamName, pub phase: PhaseId, pub hook: HookName,
    pub module_path: PathBuf, pub cwd: PathBuf, pub timeout_ms: u32,
    pub custom: serde_json::Map<String, serde_json::Value>,   // registration-time custom JSON object (Rand 2026-10-18, design.md §2.1)
    pub active: bool, pub registered_by: AgentName,
    pub registered_at: IsoTimestamp, pub disabled_at: Option<IsoTimestamp>,
}
pub trait HookRegistrationStore: crate::contract::sealed::Sealed + Send + Sync {   // sealed like TemplateCatalogStore (contract.rs:16)
    /// Upsert on (team, phase, hook); other phases' rows are untouched, any number may be active (D-5). The caller stamps
    /// `registration.registered_at` (the route uses the daemon clock, tests pass fixed stamps); the store reads no clock.
    /// Active key: replaces module_path, cwd, timeout_ms, custom, registered_by; keeps the stored registered_at (chain position).
    /// Inactive key: as above plus registered_at from the argument, disabled_at = NULL, active = 1.
    fn register(&self, registration: HookRegistration) -> Result<HookRegistration, AtmError>;
    /// active = 0, disabled_at = at on the ACTIVE rows of (team, phase), or the one hook; an inactive row keeps its disabled_at.
    /// Returns only the rows it changed; empty when none were active.
    fn disable(&self, team: &TeamName, phase: &PhaseId, hook: Option<&HookName>, at: IsoTimestamp) -> Result<Vec<HookRegistration>, AtmError>;
    /// Every active row for (team, hook), one per phase, ordered by parsed registered_at asc then phase (D-5).
    fn active(&self, team: &TeamName, hook: &HookName) -> Result<Vec<HookRegistration>, AtmError>;
    /// Every row of the team; phase filter optional; active rows first, then registered_at, then phase.
    fn list(&self, team: &TeamName, phase: Option<&PhaseId>) -> Result<Vec<HookRegistration>, AtmError>;
    /// Every active row of every team, ordered by team, hook, parsed registered_at asc, phase: the snapshot source (D-8).
    fn active_all(&self) -> Result<Vec<HookRegistration>, AtmError>;
}
// crates/atm-storage/src/testing/hook_registration_store.rs (feature test-utils)
pub struct InMemoryHookRegistrationStore { rows: Mutex<Vec<HookRegistration>>, pub calls: AtomicUsize }   // counts every trait call (ANC-014)

// crates/atm-storage/src/cron_schedule_store.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CronScheduleId(pub u64);   // Display prints the integer
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronSchedule {
    pub id: CronScheduleId, pub generation: u32, pub team: TeamName, pub phase: PhaseId, pub name: String,
    pub cron_expr: String, pub module_path: PathBuf, pub cwd: PathBuf, pub args: Vec<String>,
    pub timeout_ms: u32, pub jitter_s: u32, pub custom: serde_json::Map<String, serde_json::Value>, pub stop: StopCondition,
    pub active: bool, pub created_by: AgentName, pub created_at: IsoTimestamp,
    pub last_run_at: Option<IsoTimestamp>, pub last_outcome: Option<String>, pub ended_at: Option<IsoTimestamp>,
}
/// design.md D-9, ADR-069 D4: exactly one per schedule; `Until { at }` maps to `until_at`, `Hook` to `stop_hook = 1`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopCondition { Until { at: chrono::DateTime<chrono::Utc> }, Hook }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewCronSchedule { pub team: TeamName, pub phase: PhaseId, pub name: String, pub cron_expr: String, pub module_path: PathBuf,
    pub cwd: PathBuf, pub args: Vec<String>, pub timeout_ms: u32, pub jitter_s: u32, pub custom: serde_json::Map<String, serde_json::Value>,
    pub stop: StopCondition, pub created_by: AgentName, pub created_at: IsoTimestamp }   // jitter_s and created_at come from AtmCron (bk-4, its injected Clock)
pub trait CronScheduleStore: crate::contract::sealed::Sealed + Send + Sync {
    /// No row for (team, phase, name): INSERT with generation = 1. An ended row: UPDATE in place in one transaction, id kept,
    /// generation + 1, every field from `schedule`, active = 1, ended_at, last_run_at, last_outcome NULL (D-i). An active row:
    /// AtmError::validation_with_recovery "cron schedule <name> already active for phase <p>", recovery "run atm cron end --phase <p> <name> first".
    fn start(&self, schedule: NewCronSchedule) -> Result<CronSchedule, AtmError>;
    /// Active rows only: active = 0, ended_at = at; returns the rows changed.
    fn end(&self, team: &TeamName, phase: &PhaseId, name: Option<&str>, at: IsoTimestamp) -> Result<Vec<CronSchedule>, AtmError>;
    /// Active rows first, then parsed created_at, then phase, then name.
    fn list(&self, team: Option<&TeamName>, phase: Option<&PhaseId>) -> Result<Vec<CronSchedule>, AtmError>;
    fn active_all(&self) -> Result<Vec<CronSchedule>, AtmError>;   // daemon start (ANC-020)
    /// UPDATE .. WHERE id = ? AND generation = ? AND active = 1; Ok(false) when the row was ended or restarted since the run began,
    /// so a run that outlives its slot never writes the restarted row (the guarantee is this WHERE clause, not task abort).
    fn record_run(&self, id: CronScheduleId, generation: u32, at: IsoTimestamp, outcome: &str) -> Result<bool, AtmError>;
    /// The stop condition fired (ADR-069 D4): active = 0, ended_at = at, last_outcome = "stopped: <reason>", same WHERE clause.
    fn record_stop(&self, id: CronScheduleId, generation: u32, at: IsoTimestamp, reason: &str) -> Result<bool, AtmError>;
}
// crates/atm-storage/src/testing/cron_schedule_store.rs (feature test-utils)
pub struct InMemoryCronScheduleStore { rows: Mutex<Vec<CronSchedule>>, pub calls: AtomicUsize, pub writes: AtomicUsize,
    pub fail_active_all: AtomicBool, pub fail_writes: AtomicBool }
// calls: every trait call; writes: start, end, record_run, record_stop only; fail_active_all / fail_writes: those calls return
// AtmError::internal (the bk-4 store-error and bk-5 initial-load tests)

// crates/atm-storage/src/request_budget.rs
pub const HOOK_ACTION_RESERVE_MS: u64 = 500;
pub const HOOK_TIMEOUT_MAX_MS: u32 = 2500;   // task hooks: SERVER_REQUEST_BUDGET (3 s) minus the reserve
pub const CRON_TIMEOUT_MAX_MS: u32 = 30000;  // cron.tick / cron.stop (Rand 2026-10-18, design.md §9.8)
const _: () = assert!(HOOK_TIMEOUT_MAX_MS as u128 + HOOK_ACTION_RESERVE_MS as u128 == SERVER_REQUEST_BUDGET.as_millis());

// crates/atm-storage/src/hook_error.rs
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookExitCause { Exit { code: i32, stderr_tail: String }, Signal(i32), Spawn { path: PathBuf, cause: String }, Panic(String), Missing(String) }
impl AtmError {
    pub fn hook_exit(cause: HookExitCause) -> Self;  pub fn hook_timeout(phase: &PhaseId, detail: &str) -> Self;
    pub fn hook_malformed(detail: &str) -> Self;     pub fn hook_module_invalid(detail: &str) -> Self;
    pub fn hook_refused(message: &str) -> Self;      pub fn cron_expr_invalid(detail: &str) -> Self;
}
```

Error codes (`design.md` §9.1). The HTTP status follows `is_validation`
(400 when true, 503 otherwise, never 500); bk-7 tests the mapping.

| Code | Display prefix | `is_validation` | §7.1 class | CLI exit |
| --- | --- | --- | --- | --- |
| `ATM_HOOK_MODULE_INVALID` | `HOOK.MODULE_INVALID:` | true | operator_actionable | 3 |
| `ATM_CRON_EXPR_INVALID` | `CRON.EXPR_INVALID:` | true | operator_actionable | 3 |
| `ATM_HOOK_REFUSED` | none: the hook's message verbatim | true | operator_actionable | 3 |
| `ATM_HOOK_EXIT` | `HOOK.EXIT:` | false | retryable | 1 |
| `ATM_HOOK_TIMEOUT` | `HOOK.TIMEOUT:` | false | retryable | 1 |
| `ATM_HOOK_MALFORMED` | `HOOK.MALFORMED:` | false | retryable | 1 |

Constructor messages: `hook_exit` gives `HOOK.EXIT: exit <code>: <tail>`
(`Exit`), `HOOK.EXIT: terminated by signal <n>` (`Signal`), `HOOK.EXIT:
cannot run <path>: <cause>` (`Spawn`), `HOOK.EXIT: hook runner panicked:
<payload>` (`Panic`), `HOOK.EXIT: <what>` (`Missing`, e.g. `python3 not
found on PATH`); `hook_timeout` gives `HOOK.TIMEOUT: phase <p>: <detail>`;
`hook_malformed`, `hook_module_invalid`, `cron_expr_invalid` give their
prefix then the detail; `hook_refused` gives the message alone, with no
`Recovery:` line (empty catalog guidance).

## Authoritative deliverables

1. `atm-storage`: the traits, rows, ids and `StopCondition` above, both
   traits sealed with `crate::contract::sealed::Sealed`; each double in its
   own file under `src/testing/` behind `feature = "test-utils"`; the three
   budget constants and the `const _` assertion. Stores read no clock:
   every timestamp is an argument.
2. Errors: the six variants, their `is_validation` arms, catalog guidance,
   `HookExitCause` and the constructors in `hook_error.rs`, and
   `docs/atm-error-codes.md` §5.11 and §7.1 rows exactly as the tables
   above, in the sprint's first commit (later sprints raise these codes).
3. Boundary manifests for the two store boundaries: `io_forbidden`
   includes `process_spawn`; `allowed_test_double_paths =
   ["crate::testing::hook_registration_store::InMemoryHookRegistrationStore"]`
   and `["crate::testing::cron_schedule_store::InMemoryCronScheduleStore"]`
   (the defining module path the boundary lint matches); `allowed_dependents
   = ["atm-core", "atm-daemon-bootstrap", "atm-runtime",
   "atm-runtime-test-support", "atm-storage-rusqlite"]` (no `atm-cron`: it
   reaches the stores only through `atm_core::boundary`).
   `docs/atm-storage/boundaries.md` gains the two sections and ADR-036 the
   amendment paragraph.
4. Tests, same unit, in the files named by each criterion; the doubles'
   contract tests run against `InMemory*` here and bk-6 runs the same
   cases against SQLite.

## Authoritative acceptance criteria

All in `crates/atm-storage/tests/hook_stores_contract.rs` unless stated, against the in-memory doubles with fixed `IsoTimestamp` stamps.

- [ ] (ANC-002) `boundary:BOUNDARY-HookRegistrationStore` `disable_keeps_rows_inactive`: `disable(team, phase, None, t1)` leaves the rows present with `active == false` and `disabled_at == Some(t1)`; `active(team, hook)` then returns an empty `Vec`; `list` still returns them.
- [ ] (D-5) `boundary:BOUNDARY-HookRegistrationStore` `register_second_phase_same_hook_succeeds`: `task.close` for `bk` (stamp t2) while `bj`'s (stamp t1) is active: both active, `active(team, task.close) == [bj, bk]`; `disable(bj)` leaves `[bk]`; `reregister_active_key_keeps_chain_position`: re-registering `bj` at t3 with a new `module_path` keeps `registered_at == t1`; re-registering a disabled key at t4 stores `registered_at == t4`, clears `disabled_at` and lists last; `disable_single_hook_leaves_other_hooks_of_phase_active`.
- [ ] (REQ-P-HOOK-003 §23.1) `boundary:BOUNDARY-HookRegistrationStore` `list_orders_active_first_then_registered_at_then_phase`: disabled `bj` (t1), active `bl` (t2), `bk` (t3) list as `[bl, bk, bj]`; two active rows with one stamp list by phase; `list_schedules_orders_active_first_then_created_at_then_phase` does the same for schedules.
- [ ] `boundary:BOUNDARY-HookRegistrationStore` `disable_twice_keeps_first_disabled_at` (second call returns `[]`, `disabled_at` unchanged) and `disable_phase_with_no_rows_returns_empty`.
- [ ] (ADR-068 D3) `boundary:BOUNDARY-HookRegistrationStore` `hook_name_parse_sets`: `parse_task_hook` accepts exactly the six task names and refuses `cron.tick`, `cron.stop`, `task.foo` with `ATM_HOOK_MODULE_INVALID`; `parse` accepts all eight and refuses `task.foo` with `ATM_HOOK_MALFORMED`; `hook_name_deserialize_rejects_unknown` (`"\"task.foo\""` is `Err`).
- [ ] `boundary:BOUNDARY-HookRegistrationStore` `phase_id_rejects_empty_uppercase_space_and_newline`: accepts `bk`, `bk-2`, a 32-character id; refuses `""`, `Bk`, `b k`, `bk\n`, `2bk`, a 33-character id, each with code `ATM_MESSAGE_VALIDATION_FAILED`, the grammar in the message and the recovery line.
- [ ] (D-8) `boundary:BOUNDARY-HookRegistrationStore` `active_all_spans_teams_and_counts_calls`: two teams' active rows, disabled rows omitted, `calls` counts the call; `active_all_order_equals_active_order_for_every_key`.
- [ ] (ANC-018, ANC-020) `boundary:BOUNDARY-CronScheduleStore` `active_all_returns_started_rows_with_cron_expr_byte_identical` and `jitter_custom_and_created_at_round_trip` (`jitter_s = 1799`, `custom = {"goal":"x","n":[1,{"y":null}]}`, `created_at` equal to the `NewCronSchedule` value).
- [ ] (ANC-023) `boundary:BOUNDARY-CronScheduleStore` `new_cron_schedule_requires_a_stop_condition`: `NewCronSchedule.stop` is a `StopCondition`, not an `Option`; `StopCondition` serializes as `{"kind":"until","at":".."}` and `{"kind":"hook"}`.
- [ ] (ANC-024, ANC-025) `boundary:BOUNDARY-CronScheduleStore` `record_stop_ends_the_row_once`: `record_stop(id, 1, t, "phase bead closed")` gives `active == false`, `ended_at == t`, `last_outcome == "stopped: phase bead closed"`, `active_all()` omits it; a second call returns `Ok(false)`.
- [ ] (D-i) `boundary:BOUNDARY-CronScheduleStore` `start_after_end_updates_in_place_and_bumps_generation`: same `id`, `generation == 2`, `active`, `created_at` from the new spec, `ended_at`/`last_run_at`/`last_outcome` `None`; `start_refuses_active_duplicate` (code `ATM_MESSAGE_VALIDATION_FAILED`, recovery names `atm cron end`); the refused start adds to `calls` and not to `writes`.
- [ ] (D-i stale run) `boundary:BOUNDARY-CronScheduleStore` `record_with_stale_generation_changes_nothing`: after end and restart (`generation == 2`), `record_run(id, 1, t, "ok")` and `record_stop(id, 1, ..)` return `Ok(false)` and the row is unchanged; `record_run_on_ended_row_changes_nothing`.
- [ ] (REQ-BK-ERRORS) `boundary:BOUNDARY-AtmErrorCodes` `crates/atm-error/src/lib.rs` round-trip tables carry the six codes through `as_str`/`from_str`; `crates/atm-storage/src/hook_error.rs::tests::hook_error_constructors_format_and_classify` asserts all five `HookExitCause` messages and the other five constructors' messages, `is_validation` true for exactly the three validation codes, and `hook_refused("bd not ready")` displays exactly `bd not ready`; `docs/atm-error-codes.md` lists each code once in §5.11 and once in §7.1.
- [ ] (ANC-011) `boundary:BOUNDARY-HookRegistrationStore` `HookRegistration.timeout_ms` is `u32`; `HOOK_TIMEOUT_MAX_MS == 2500`, `HOOK_ACTION_RESERVE_MS == 500`, `CRON_TIMEOUT_MAX_MS == 30000`, and the `const _` assertion ties the first two to `SERVER_REQUEST_BUDGET` (a mismatch fails the build).
- [ ] `boundary:BOUNDARY-HookRegistrationStore` both traits sealed, doubles compiled only under `test-utils`; `just lint` passes the two manifests with each double's defining path as the sole test double; `docs/atm-storage/boundaries.md` has one section per store and ADR-036 names both under ADR-069 D5.

## Authoritative validation

- `cargo build --workspace`
- `cargo test -p atm-storage --features test-utils -p atm-error`
- `cargo clippy -p atm-storage -p atm-error --all-targets --features atm-storage/test-utils -- -D warnings`
- `just lint` (boundary manifests; `.just/check_line_counts.py`: `testing.rs` ≤ 913, `error.rs` ≤ 465 production lines)

## Non-closure

- No SQLite table, rusqlite store or `StorageHandles` accessor (bk-6); no
  core hook type, sealed core trait or doubles re-export (bk-2); no process
  is spawned (bk-3); no route or protocol variant (bk-7); no tick (bk-4);
  no CLI command or exit-code arm (bk-5).
