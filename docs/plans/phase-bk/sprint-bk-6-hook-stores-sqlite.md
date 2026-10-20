# Sprint bk-6 — SQLite hook registration and cron schedule stores

closure_type `boundary`; target_boundary
`BOUNDARY-HookRegistrationStore-Sqlite` and
`BOUNDARY-CronScheduleStore-Sqlite` (atm-storage-rusqlite, new) with the
`hook_registration` and `cron_schedule` DDL; branch
`sprint/bk-6-hook-stores-sqlite`, PR → `sprint/bk-2-hook-contract` (layer
3); difficulty normal; recommended_agent `arch-ctm`, recommended_model
`gpt-5-codex`. Wave 2, developed from bk-1's head and rebased onto bk-2's layer
when both are done (parallel-then-rebase, Rand
2026-10-12). Consumes from bk-1: `HookRegistrationStore`,
`CronScheduleStore`, `HookRegistration`, `CronSchedule`,
`NewCronSchedule`, `StopCondition`, `CronScheduleId`, `HookName`,
`PhaseId` and the error constructors. Governing text: `design.md` §4, D-5,
D-8, D-9, D-i; anchors ANC-002, ANC-018, ANC-020, ANC-023, REQ-BK-SCHEMA.
Requirements `REQ-CORE-HOOK-002` (requirements.md:5403),
`REQ-P-HOOK-001` §23.2, `REQ-P-CRON-001` §24.1; ADR-061 D1, D3, D6
(record of 2026-10-18), ADR-069 D1, D4.

## Scope boundary

This sprint owns the durable implementations of bk-1's two store traits
and their schema: the rusqlite stores, the DDL wired into `ensure_schema`,
the factory composition and the two `StorageHandles` accessors, and the
two rusqlite boundary manifests. The accessors live in
`crates/atm-storage/src/factory.rs` because `StorageHandleParts` has one
construction site, `SqliteStorageFactory::open`
(`crates/atm-storage-rusqlite/src/lib.rs:819`), which must set the new
fields in the same commit. It edits no other file under
`crates/atm-storage` and none under `crates/atm-core`, `crates/atm-runtime`
or the runtime crates; installing the stores into `LocalServiceRuntime`
is bk-5's composition.

Owned paths (new unless marked edited):

- `crates/atm-storage-rusqlite/src/hook_registration_store.rs`,
  `crates/atm-storage-rusqlite/src/cron_schedule_store.rs`,
  `crates/atm-storage-rusqlite/src/hook_schema.rs`,
  `crates/atm-storage-rusqlite/src/shared_db.rs` (edited: one
  `crate::hook_schema::ensure_schema(connection, target)?;` line after
  `crate::task_store::ensure_schema` at `shared_db.rs:677`; 796 production
  lines),
  `crates/atm-storage-rusqlite/src/lib.rs` (edited: two `mod` lines, two
  backend fields and the two `StorageHandleParts` fields at `:819`; 826
  production lines, so about 832 after),
  `crates/atm-storage-rusqlite/tests/hook_registration_store.rs`,
  `crates/atm-storage-rusqlite/tests/cron_schedule_store.rs`,
  `crates/atm-storage-rusqlite/tests/hook_schema.rs`
- `crates/atm-storage/src/factory.rs` (edited: the two fields on
  `StorageHandleParts` (`:132`) and `StorageHandles`, and accessors
  `hook_registration_store()` and `cron_schedule_store()` after
  `task_store()` (`:246`); 203 production lines)
- `boundaries/atm-storage-rusqlite/hook-registration-store-sqlite.toml`,
  `boundaries/atm-storage-rusqlite/cron-schedule-store-sqlite.toml`

## Public contract

```rust
// crates/atm-storage-rusqlite: pub(crate)-constructed, private outside the crate, exactly as SqliteTaskStore (task-store-sqlite.toml)
pub(crate) struct SqliteHookRegistrationStore { db: SharedDb }   // register: INSERT .. ON CONFLICT(team, phase, hook) DO UPDATE (an upsert, never INSERT OR REPLACE)
pub(crate) struct SqliteCronScheduleStore { db: SharedDb }       // start-after-end: one UPDATE in the writer transaction, generation = generation + 1
impl atm_storage::contract::sealed::Sealed for SqliteHookRegistrationStore {}  impl HookRegistrationStore for SqliteHookRegistrationStore {}
impl atm_storage::contract::sealed::Sealed for SqliteCronScheduleStore {}      impl CronScheduleStore for SqliteCronScheduleStore {}
pub(crate) fn ensure_schema(connection: &Connection, target: &Path) -> Result<(), AtmError>;   // hook_schema.rs, both CREATE TABLE IF NOT EXISTS

// crates/atm-storage/src/factory.rs
impl StorageHandles {
    pub fn hook_registration_store(&self) -> Arc<dyn HookRegistrationStore + Send + Sync>;
    pub fn cron_schedule_store(&self) -> Arc<dyn CronScheduleStore + Send + Sync>;
}
```

A UNIQUE, CHECK or NOT NULL violation maps to
`AtmError::validation_with_recovery` (code `ATM_MESSAGE_VALIDATION_FAILED`)
naming the table and constraint, recovery "check the request fields
against docs/atm-storage/boundaries.md"; the active-duplicate `start`
refusal is bk-1's message, raised before the INSERT. A stored `custom`
that is not a JSON object, or `args` that is not a JSON string array, is
`AtmError::internal` naming the row id.

DDL (`hook_schema.rs`, `CREATE TABLE IF NOT EXISTS` like `prompt_handoffs`):

```sql
CREATE TABLE IF NOT EXISTS hook_registration (
    team TEXT NOT NULL, phase TEXT NOT NULL, hook TEXT NOT NULL,
    module_path TEXT NOT NULL, cwd TEXT NOT NULL, timeout_ms INTEGER NOT NULL CHECK(timeout_ms > 0),
    custom TEXT NOT NULL DEFAULT '{}', active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    registered_by TEXT NOT NULL, registered_at TEXT NOT NULL, disabled_at TEXT NULL,
    CHECK ((active = 1) = (disabled_at IS NULL)),
    PRIMARY KEY (team, phase, hook));
CREATE TABLE IF NOT EXISTS cron_schedule (
    id INTEGER PRIMARY KEY AUTOINCREMENT, generation INTEGER NOT NULL DEFAULT 1 CHECK(generation >= 1),
    team TEXT NOT NULL, phase TEXT NOT NULL, name TEXT NOT NULL,
    cron_expr TEXT NOT NULL, module_path TEXT NOT NULL, cwd TEXT NOT NULL, args TEXT NOT NULL,
    timeout_ms INTEGER NOT NULL CHECK(timeout_ms > 0), jitter_s INTEGER NOT NULL DEFAULT 0 CHECK(jitter_s >= 0),
    custom TEXT NOT NULL DEFAULT '{}', active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    until_at TEXT NULL, stop_hook INTEGER NOT NULL DEFAULT 0 CHECK(stop_hook IN (0, 1)),
    created_by TEXT NOT NULL, created_at TEXT NOT NULL, last_run_at TEXT NULL, last_outcome TEXT NULL, ended_at TEXT NULL,
    CHECK ((until_at IS NOT NULL) + (stop_hook = 1) = 1),
    CHECK ((active = 1) = (ended_at IS NULL)),
    UNIQUE (team, phase, name));
```

`args` is a JSON array string and `custom` a JSON object string; `until_at`
is RFC 3339 UTC text. The stop CHECK enforces D-9 (exactly one stop
condition); the two lifecycle CHECKs make an active row with a
`disabled_at`/`ended_at`, or an inactive row without one, unstorable.
`record_run` and `record_stop` are `UPDATE .. WHERE id = ?1 AND
generation = ?2 AND active = 1`. Both tables are additive SQLite MINOR
objects (ADR-061 D1): no foreign key, created idempotently, ignored by a
pre-bk binary. The previous-consumer proof is the pre-bk DDL test below.

Manifests (format of `task-store-sqlite.toml`): `[implementation]
visibility = "private"`, `constructor = "pub(crate)"`, `module =
"atm_storage_rusqlite::hook_registration_store"` /
`"atm_storage_rusqlite::cron_schedule_store"`; `[composition] roots =
["atm_storage_rusqlite::SqliteStorageBackend::new"]`; `io_owns =
["sqlite"]`, `io_forbidden = ["message_delivery", "process_spawn"]`;
`allowed_dependents = ["atm-daemon-bootstrap",
"atm-runtime-test-support"]`, `allowed_dependencies = ["atm-storage",
"rusqlite"]`, `forbidden_edges` the three of `task-store-sqlite.toml`;
`[references] forbidden = ["SqliteHookRegistrationStore",
"rusqlite::Connection"]` (resp. `SqliteCronScheduleStore`);
`allowed_test_double_paths = []`.

## Authoritative deliverables

1. `hook_schema.rs` DDL wired into `ensure_schema` (one line at
   `shared_db.rs:677`).
2. `SqliteHookRegistrationStore` and `SqliteCronScheduleStore` on the
   shared writer connection, implementing every rule of bk-1's trait docs;
   `StopCondition::Until { at }` stored as `until_at` with `stop_hook = 0`,
   `Hook` as `until_at NULL`, `stop_hook = 1`; constraint violations mapped
   as above.
3. `SqliteStorageFactory::open` composes both; `StorageHandles` exposes
   both accessors.
4. The two manifests above.
5. Tests, same unit, in the files the criteria name, on a temp-dir
   database with fixed timestamp arguments.

## Authoritative acceptance criteria

`crates/atm-storage-rusqlite/tests/hook_registration_store.rs`:

- [ ] (ANC-002, D-5) `boundary:BOUNDARY-HookRegistrationStore-Sqlite` `disable_keeps_rows_inactive`, `register_second_phase_same_hook_succeeds`, `reregister_active_key_keeps_chain_position`, `list_orders_active_first_then_registered_at_then_phase`, `disable_twice_keeps_first_disabled_at`: bk-1's cases with the same fixed stamps give the same rows on SQLite.
- [ ] (D-8) `boundary:BOUNDARY-HookRegistrationStore-Sqlite` `registrations_survive_reopen`: rows registered on one backend are returned by `active_all()` and `list` on a second backend opened on the same file, in `active_all` order, field by field; `register_is_an_upsert_not_a_replace` (re-registering an active key keeps `registered_at` in the row, read back by SQL).
- [ ] (ANC-011, Rand 2026-10-18 custom) `boundary:BOUNDARY-HookRegistrationStore-Sqlite` `zero_timeout_fails_the_check` (`ATM_MESSAGE_VALIDATION_FAILED` naming `hook_registration` and `timeout_ms`); `custom_round_trips_and_defaults_to_empty_object` (a row inserted by SQL without `custom` reads back as an empty map).

`crates/atm-storage-rusqlite/tests/cron_schedule_store.rs`:

- [ ] (ANC-018, ANC-020) `boundary:BOUNDARY-CronScheduleStore-Sqlite` `active_all_survives_reopen`: a schedule started on one backend is returned by `active_all()` on a second backend opened on the same file (the daemon-restart case: bk-4's worker loads exactly `active_all()`), `cron_expr` byte-identical, `stop`, `jitter_s`, `custom`, `generation` equal.
- [ ] (ANC-023) `boundary:BOUNDARY-CronScheduleStore-Sqlite` `stop_condition_check_and_round_trip`: SQL inserts with `until_at NULL, stop_hook 0` and with both set each fail the CHECK; `Until` round-trips as RFC 3339 text and `Hook` as `stop_hook = 1`.
- [ ] (D-i) `boundary:BOUNDARY-CronScheduleStore-Sqlite` `start_after_end_updates_in_place_and_bumps_generation`, `start_refuses_active_duplicate`, `record_with_stale_generation_changes_nothing` (after a restart, `record_run(id, 1, ..)` and `record_stop(id, 1, ..)` return `Ok(false)` and the row's `last_outcome` stays `NULL`), `record_stop_ends_the_row_once`, `record_run_on_ended_row_changes_nothing`.
- [ ] (rust-best-practices lifecycle) `boundary:BOUNDARY-CronScheduleStore-Sqlite` `lifecycle_checks_refuse_contradictory_rows`: SQL inserts of an active `hook_registration` row with `disabled_at` set, an inactive one without, an active `cron_schedule` row with `ended_at` set and an inactive one without each fail with a CHECK error.

`crates/atm-storage-rusqlite/tests/hook_schema.rs`:

- [ ] (REQ-BK-SCHEMA) `boundary:BOUNDARY-SharedDb-Sqlite` `pre_bk_ddl_set_reads_and_writes_after_hook_tables_created`: both tables are created by `ensure_schema` on an empty database and on one created by the pre-bk `DB_MIGRATIONS` string; the frozen pre-bk statements still read and write afterwards (pattern `pre_bb_ddl_set_reads_and_writes_after_prompt_handoffs_created`, `tests/prompt_handoffs.rs:415`); `ensure_schema_twice_is_idempotent`; `hook_tables_have_no_sha_column` (D-2, ANC-016): no column (`PRAGMA table_info`) has a name containing `sha`.
- [ ] `boundary:BOUNDARY-SharedDb-Sqlite` `storage_handles_expose_both_stores`: `SqliteStorageFactory::open` on a temp dir returns handles whose two accessors round-trip one row each.
- [ ] `boundary:BOUNDARY-HookRegistrationStore-Sqlite` the rusqlite types are not constructible outside their crate; `just lint` passes the two manifests.

## Authoritative validation

- `cargo build --workspace`
- `cargo test -p atm-storage-rusqlite -p atm-storage --features atm-storage/test-utils`
- `cargo clippy -p atm-storage-rusqlite -p atm-storage --all-targets -- -D warnings`
- `just lint` (boundary manifests; `.just/check_line_counts.py`: `lib.rs` and `shared_db.rs` stay under 1000 production lines)

## Non-closure

- No `LocalServiceRuntime` install of the stores (bk-5 composition); no
  core type, route, runner, scheduler or CLI.

## Store implementation note

`CronScheduleStore::end` and `HookRegistrationStore::disable` keep their
select-update-patch transactions in their respective store modules. They use
different selectors and ordering keys, return different row types, and update
different lifecycle columns. A shared helper in `store_support` would need
table-specific SQL identifiers and generic row decoding or callbacks, making
the storage abstraction less clear than the small duplicated transaction
shape.
