# Sprint bk-1 — hook registration and cron schedule stores

closure_type `contract`; target_boundary `BOUNDARY-HookRegistrationStore`,
`BOUNDARY-CronScheduleStore` (atm-storage, new; rusqlite implementations);
branch `sprint/bk-1-hook-stores`, PR → `integrate/phase-bk`; difficulty
normal. Governing text: `design.md` §4, D-5, D-7, D-8, D-9; anchors ANC-002,
ANC-018, ANC-020, ANC-023, REQ-BK-SCHEMA. Requirements `REQ-P-HOOK-001`
§23.2 (one active registration per team and hook), `REQ-P-CRON-001` §24.1
(the table); ADR-068 D1, D5, ADR-069 D1, D4 (the columns), ADR-061 D3 and D6
(record of 2026-10-18 already on this branch), ADR-063 D3 as amended by
ADR-069 D5 (fourteen capabilities; Rand round 5: "yes, we need storage for
cron").

## Scope boundary

This sprint owns the two storage capabilities every other sprint reads or
writes through, and nothing above `atm-storage`: the store traits, their
row types, the in-memory doubles, the rusqlite implementations and DDL, the
storage boundary manifests and the storage documentation. It edits no file
under `crates/atm-core`, `crates/atm-error`, `crates/atm-http-runtime`,
`crates/atm-daemon-bootstrap` or `crates/atm`; the core hook types, the
sealed traits and the protocol variants are bk-2's.

Owned paths (new unless marked edited):

- `crates/atm-storage/src/hook_registration_store.rs`,
  `crates/atm-storage/src/cron_schedule_store.rs`,
  `crates/atm-storage/src/lib.rs` (edited: two `pub mod` + re-exports),
  `crates/atm-storage/src/factory.rs` (edited: two handles on
  `StorageHandleParts`/`StorageHandles` + accessors
  `hook_registration_store()` and `cron_schedule_store()` next to
  `task_store()`, `factory.rs:246`),
  `crates/atm-storage/src/testing.rs` (edited: the in-memory doubles
  behind `feature = "test-utils"`)
- `crates/atm-storage-rusqlite/src/hook_registration_store.rs`,
  `crates/atm-storage-rusqlite/src/cron_schedule_store.rs`,
  `crates/atm-storage-rusqlite/src/hook_schema.rs`,
  `crates/atm-storage-rusqlite/src/shared_db.rs` (edited: `ensure_schema`
  call at the line-654 block), `crates/atm-storage-rusqlite/src/lib.rs`
  (edited: mods + factory wiring at `impl StorageFactory for
  SqliteStorageFactory`, line 774)
- `boundaries/atm-storage/hook-registration-store.toml`,
  `boundaries/atm-storage/cron-schedule-store.toml`,
  `boundaries/atm-storage-rusqlite/hook-registration-store-sqlite.toml`,
  `boundaries/atm-storage-rusqlite/cron-schedule-store-sqlite.toml`
  (format of `boundaries/atm-storage/task-store.toml` and
  `task-store-sqlite.toml`)
- `docs/atm-storage/boundaries.md` (edited: `## HookRegistrationStore`
  and `## CronScheduleStore` sections after `## TaskStore`, format of that
  file), `docs/adr/ADR-036-*.md` (one amendment paragraph, format of its
  Phase AQ amendment, naming the two capabilities under ADR-069 D5)

## Public contract

```rust
// crates/atm-storage/src/hook_registration_store.rs
pub struct HookName(String);            // task.assign|task.start|task.close|task.move|task.list|task.events|cron.tick|cron.stop
impl HookName { pub const TASK_HOOKS: [&'static str; 6]; pub const CRON_TICK: &'static str = "cron.tick"; pub const CRON_STOP: &'static str = "cron.stop";
    pub fn parse(&str) -> Result<Self, AtmError>;              // any of the eight names, else HookMalformed
    pub fn parse_task_hook(&str) -> Result<Self, AtmError>;    // exactly TASK_HOOKS; cron.* and unknown names => HookModuleInvalid (the register route's only validator, ADR-068 D3)
    pub fn is_task_hook(&self) -> bool; }
pub struct PhaseId(String);             // lower-case phase id, e.g. "bk"
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRegistration {
    pub team: TeamName, pub phase: PhaseId, pub hook: HookName,
    pub module_path: PathBuf, pub cwd: PathBuf, pub timeout_ms: u32,
    pub active: bool, pub registered_by: AgentName,
    pub registered_at: IsoTimestamp, pub disabled_at: Option<IsoTimestamp>,
}
pub trait HookRegistrationStore: crate::contract::sealed::Sealed + Send + Sync {   // sealed like TemplateCatalogStore (contract.rs:16)
    /// Upsert on (team, phase, hook); Err(HookActive{phase}) when another phase's row for (team, hook) is active (D-5).
    fn register(&self, registration: HookRegistration) -> Result<HookRegistration, AtmError>;
    /// Sets active=0, disabled_at=now on every row of (team, phase), or the one hook; returns the rows changed.
    fn disable(&self, team: &TeamName, phase: &PhaseId, hook: Option<&HookName>) -> Result<Vec<HookRegistration>, AtmError>;
    /// The single active row for (team, hook), if any (D-5 guarantees at most one).
    fn active(&self, team: &TeamName, hook: &HookName) -> Result<Option<HookRegistration>, AtmError>;
    /// Every row of the team (doctor, `atm hook list`); phase filter optional.
    fn list(&self, team: &TeamName, phase: Option<&PhaseId>) -> Result<Vec<HookRegistration>, AtmError>;
    /// Every active row of every team: the router's in-memory snapshot source (design D-8).
    fn active_all(&self) -> Result<Vec<HookRegistration>, AtmError>;
}
// crates/atm-storage/src/testing.rs (feature test-utils; re-exported by atm-runtime-test-support in bk-2)
pub struct InMemoryHookRegistrationStore { rows: Mutex<Vec<HookRegistration>>, pub calls: AtomicUsize }   // calls counts every trait call (ANC-014 proof)

// crates/atm-storage/src/cron_schedule_store.rs
pub struct CronScheduleId(u64);
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronSchedule {
    pub id: CronScheduleId, pub team: TeamName, pub phase: PhaseId, pub name: String,
    pub cron_expr: String, pub module_path: PathBuf, pub cwd: PathBuf, pub args: Vec<String>,
    pub timeout_ms: u32, pub stop: StopCondition, pub active: bool, pub created_by: AgentName, pub created_at: IsoTimestamp,
    pub last_run_at: Option<IsoTimestamp>, pub last_outcome: Option<String>, pub ended_at: Option<IsoTimestamp>,
}
/// design.md D-9, ADR-069 D4: exactly one per schedule; `Until` maps to the `until_at` column, `Hook` to `stop_hook = 1`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopCondition { Until { at: chrono::DateTime<chrono::Utc> }, Hook }
pub struct NewCronSchedule { pub team: TeamName, pub phase: PhaseId, pub name: String, pub cron_expr: String, pub module_path: PathBuf, pub cwd: PathBuf, pub args: Vec<String>, pub timeout_ms: u32, pub stop: StopCondition, pub created_by: AgentName }
pub trait CronScheduleStore: crate::contract::sealed::Sealed + Send + Sync {
    fn start(&self, schedule: NewCronSchedule) -> Result<CronSchedule, AtmError>;        // UNIQUE(team, phase, name): Err on an active duplicate
    fn end(&self, team: &TeamName, phase: &PhaseId, name: Option<&str>) -> Result<Vec<CronSchedule>, AtmError>;
    fn list(&self, team: Option<&TeamName>, phase: Option<&PhaseId>) -> Result<Vec<CronSchedule>, AtmError>;
    fn active_all(&self) -> Result<Vec<CronSchedule>, AtmError>;                          // daemon start (ANC-020)
    fn record_run(&self, id: CronScheduleId, at: IsoTimestamp, outcome: &str) -> Result<(), AtmError>;
    /// The stop condition fired (ADR-069 D4): active = 0, ended_at = at, last_outcome = "stopped: <reason>"; Ok(false) when the row was already inactive.
    fn record_stop(&self, id: CronScheduleId, at: IsoTimestamp, reason: &str) -> Result<bool, AtmError>;
}
// crates/atm-storage/src/testing.rs
pub struct InMemoryCronScheduleStore { rows: Mutex<Vec<CronSchedule>>, pub calls: AtomicUsize }

// crates/atm-storage-rusqlite: pub(crate)-constructed, private outside the crate, exactly as SqliteTaskStore (task-store-sqlite.toml)
pub(crate) struct SqliteHookRegistrationStore { db: SharedDb }   // register: one transaction: SELECT active rows of (team, hook) in other phases, Err(HookActive{phase}) if any, else INSERT OR REPLACE
pub(crate) struct SqliteCronScheduleStore { db: SharedDb }       // start: INSERT, the UNIQUE and CHECK constraints map to AtmError::validation naming the constraint
```

DDL (`hook_schema.rs`, run from `ensure_schema`, `CREATE TABLE IF NOT EXISTS`
like `prompt_handoffs`):

```sql
CREATE TABLE IF NOT EXISTS hook_registration (
    team TEXT NOT NULL, phase TEXT NOT NULL, hook TEXT NOT NULL,
    module_path TEXT NOT NULL, cwd TEXT NOT NULL, timeout_ms INTEGER NOT NULL CHECK(timeout_ms > 0),
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    registered_by TEXT NOT NULL, registered_at TEXT NOT NULL, disabled_at TEXT NULL,
    PRIMARY KEY (team, phase, hook));
CREATE TABLE IF NOT EXISTS cron_schedule (
    id INTEGER PRIMARY KEY AUTOINCREMENT, team TEXT NOT NULL, phase TEXT NOT NULL, name TEXT NOT NULL,
    cron_expr TEXT NOT NULL, module_path TEXT NOT NULL, cwd TEXT NOT NULL, args TEXT NOT NULL,
    timeout_ms INTEGER NOT NULL CHECK(timeout_ms > 0), active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    until_at TEXT NULL, stop_hook INTEGER NOT NULL DEFAULT 0 CHECK(stop_hook IN (0, 1)),
    created_by TEXT NOT NULL, created_at TEXT NOT NULL, last_run_at TEXT NULL, last_outcome TEXT NULL, ended_at TEXT NULL,
    CHECK ((until_at IS NOT NULL) + (stop_hook = 1) = 1),
    UNIQUE (team, phase, name));
```

`args` is a JSON array string; `until_at` is RFC 3339 UTC text; the last
CHECK enforces design D-9 (exactly one stop condition). Both tables are
additive SQLite MINOR objects (ADR-061 D1): no foreign key, created
idempotently, ignored by a pre-bk binary.

## Authoritative deliverables

1. `atm-storage`: `HookRegistrationStore` (with `active_all`),
   `HookRegistration`, `HookName` (`parse`, `parse_task_hook`,
   `is_task_hook`), `PhaseId`, `CronScheduleStore` (with `record_stop`),
   `CronSchedule`, `NewCronSchedule`, `StopCondition`, `CronScheduleId` as
   above, both traits sealed with `crate::contract::sealed::Sealed`; the
   in-memory doubles in `atm_storage::testing` behind
   `feature = "test-utils"` with a call counter;
   `StorageHandleParts`/`StorageHandles` gain `hook_registration_store` and
   `cron_schedule_store` with accessors next to `task_store()`
   (`factory.rs:246`).
2. `atm-storage-rusqlite`: `hook_schema.rs` DDL wired into `ensure_schema`
   (`shared_db.rs:654` block, after `task_store::ensure_schema`);
   `SqliteHookRegistrationStore` and `SqliteCronScheduleStore` on the shared
   writer connection, `pub(crate)`-constructed and private outside the
   crate exactly as `SqliteTaskStore`; `SqliteStorageFactory::open`
   (`lib.rs:774`) composes both. `register` enforces D-5 in one transaction
   (select active rows of (team, hook) in other phases, then upsert).
   `StopCondition::Until { at }` is stored as `until_at` RFC 3339 text with
   `stop_hook = 0`, `Hook` as `until_at NULL`, `stop_hook = 1`, and read back
   the same way. Rows persist across reopen (ANC-002, ANC-018, ANC-020).
3. Boundary manifests for the four store boundaries: `io_forbidden`
   includes `process_spawn` on both traits; the two `atm-storage` manifests
   list `allowed_test_double_paths =
   ["atm_storage::testing::InMemoryHookRegistrationStore"]` /
   `["atm_storage::testing::InMemoryCronScheduleStore"]` and
   `allowed_dependents = ["atm-core", "atm-cron", "atm-daemon-bootstrap",
   "atm-runtime-test-support", "atm-storage-rusqlite"]`; the rusqlite
   manifests `constructor = "pub(crate)"`, `visibility = "private"`.
   `docs/atm-storage/boundaries.md` gains the two store sections and
   ADR-036 the amendment paragraph; ADR-069 D5 is the authority for both.
4. Tests, same unit: `atm-storage` contract tests over the in-memory
   doubles; `atm-storage-rusqlite` tests on a temp database:
   register/disable/active/list round trip, D-5 refusal naming the other
   phase, `parse_task_hook` on the eight names and two unknown ones, cron
   start duplicate refusal, a row with neither or both stop conditions
   refused by the CHECK, `StopCondition` round trip for both variants,
   `record_stop` (`active`, `ended_at`, `last_outcome` set; `Ok(false)` on
   an inactive row), `active_all` after reopen, `record_run`, pre-bk DDL
   set still reads and writes after both tables exist (ADR-061 D3 proof,
   pattern `pre_bb_ddl_set_reads_and_writes_after_prompt_handoffs_created`).

## Authoritative acceptance criteria

- [ ] (ANC-002) `boundary:HookRegistrationStore` `disable(team, phase, None)` leaves the rows present with `active == false` and `disabled_at` set; `active(team, hook)` then returns `None`; `list` still returns them.
- [ ] (D-5) `boundary:HookRegistrationStore` registering `task.close` for phase `bk` while phase `bj`'s `task.close` is active fails with `ATM_HOOK_ACTIVE` whose message names `bj`; after `disable(bj)` the same call succeeds.
- [ ] (ADR-068 D3) `boundary:HookRegistrationStore` `HookName::parse_task_hook` accepts exactly the six task names and refuses `cron.tick`, `cron.stop` and `task.foo` with `ATM_HOOK_MODULE_INVALID`; `HookName::parse` accepts all eight.
- [ ] (ANC-011) `boundary:HookRegistrationStore` `HookRegistration.timeout_ms` is `u32` and the DDL `CHECK(timeout_ms > 0)` rejects `0` (the value itself is read from the module by bk-5).
- [ ] (D-8) `boundary:HookRegistrationStore` `active_all()` returns the active rows of two teams and omits disabled rows; `InMemoryHookRegistrationStore.calls` counts the call.
- [ ] (ANC-018, ANC-020) `boundary:CronScheduleStore` a schedule started on one `SqliteStorageBackend` is returned by `active_all()` on a second backend opened on the same file (the daemon-restart case: bk-4's worker loads exactly `active_all()` at daemon start), with `cron_expr` byte-identical and `stop` equal to the value started.
- [ ] (ANC-023) `boundary:CronScheduleStore` `NewCronSchedule` has no constructor without a `StopCondition`; an `INSERT` with `until_at NULL` and `stop_hook 0`, and one with both set, each fail the CHECK; `StopCondition::Until` round-trips as `until_at` RFC 3339 text and `Hook` as `stop_hook = 1`.
- [ ] (ANC-024, ANC-025) `boundary:CronScheduleStore` `record_stop(id, t, "phase bead closed")` leaves the row with `active == false`, `ended_at == t`, `last_outcome == "stopped: phase bead closed"` and `active_all()` omits it; a second call returns `Ok(false)` and changes nothing.
- [ ] (REQ-BK-SCHEMA) `boundary:shared-db` both tables are created by `ensure_schema` on an empty database and on a database created by the pre-bk `DB_MIGRATIONS` string; the frozen pre-bk statements still read and write afterwards.
- [ ] `boundary:HookRegistrationStore` both store traits are sealed, their doubles are compiled only under `test-utils`, the two `atm-storage` manifests list each double as the sole test double, the rusqlite types are not constructible outside their crate; `just lint` boundary check passes with the four manifests.
- [ ] `boundary:shared-db` `docs/atm-storage/boundaries.md` has one section per new store and ADR-036 names both under ADR-069 D5; ADR-063 itself is not edited.

## Authoritative validation

- `cargo build --workspace`
- `cargo test -p atm-storage --features test-utils -p atm-storage-rusqlite`
- `cargo clippy -p atm-storage -p atm-storage-rusqlite --all-targets -- -D warnings`
- `just lint` (boundary manifests, `.just/check_line_counts.py`, RULE-003)

## Non-closure

- No core hook type, sealed trait, protocol variant or error code (bk-2:
  `HookRequest`, `HookReply`, `StopReply`, `HookRunner`, `CronScheduler`,
  `HookActionDispatcher`, the twelve envelope variants, `ATM_HOOK_*` and
  `ATM_CRON_*`); no hook process is spawned (bk-3); no tick fires (bk-4);
  no CLI command, `TIMEOUT_MS` read or doctor section (bk-5).
- `LocalServiceRuntime` accessors for the two stores are bk-2's (an
  `atm-core` file).
