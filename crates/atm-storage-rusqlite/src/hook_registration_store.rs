//! SQLite adapter for the hook registration store (ADR-068 D1, ADR-069 D1).
//!
//! Writes run in the shared writer transaction and reads run on the reader
//! pool. Timestamps are fixed-width UTC text (see `store_support`), so SQL
//! `ORDER BY` matches the chronological order the trait documents.

use std::path::PathBuf;
use std::sync::Arc;

use atm_storage::contract::sealed;
use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{AtmError, HookName, HookRegistration, HookRegistrationStore, PhaseId};
use rusqlite::types::Value;
use rusqlite::{Connection, Row, params};

use crate::shared_db::SharedDb;
use crate::store_support::{
    encode_custom, internal, parse_custom, parse_optional_stamp, parse_stamp, query_rows, stamp,
    text, write_error,
};

struct SqliteHookRegistrationStore {
    db: Arc<SharedDb>,
}

pub(crate) fn hook_registration_store(
    db: Arc<SharedDb>,
) -> Arc<dyn HookRegistrationStore + Send + Sync> {
    Arc::new(SqliteHookRegistrationStore { db })
}

impl sealed::Sealed for SqliteHookRegistrationStore {}

const TABLE: &str = "hook_registration";

const SELECT_COLUMNS: &str = "team, phase, hook, module_path, cwd, timeout_ms, custom,
    active, registered_by, registered_at, disabled_at";

/// One `hook_registration` row as stored, before the typed fields are parsed.
struct RawHook {
    team: String,
    phase: String,
    hook: String,
    module_path: String,
    cwd: String,
    timeout_ms: i64,
    custom: String,
    active: bool,
    registered_by: String,
    registered_at: String,
    disabled_at: Option<String>,
}

fn read_raw(row: &Row<'_>) -> rusqlite::Result<RawHook> {
    Ok(RawHook {
        team: row.get(0)?,
        phase: row.get(1)?,
        hook: row.get(2)?,
        module_path: row.get(3)?,
        cwd: row.get(4)?,
        timeout_ms: row.get(5)?,
        custom: row.get(6)?,
        active: row.get::<_, i64>(7)? == 1,
        registered_by: row.get(8)?,
        registered_at: row.get(9)?,
        disabled_at: row.get(10)?,
    })
}

fn decode(raw: RawHook) -> Result<HookRegistration, AtmError> {
    let key = format!("{TABLE} {}/{}/{}", raw.team, raw.phase, raw.hook);
    Ok(HookRegistration {
        team: TeamName::from_validated(raw.team),
        phase: PhaseId::parse(&raw.phase).map_err(|error| internal(&key, error.message()))?,
        hook: HookName::parse(&raw.hook).map_err(|error| internal(&key, error.message()))?,
        module_path: PathBuf::from(raw.module_path),
        cwd: PathBuf::from(raw.cwd),
        timeout_ms: u32::try_from(raw.timeout_ms)
            .map_err(|_| internal(&key, "timeout_ms is out of range"))?,
        custom: parse_custom(&key, &raw.custom)?,
        active: raw.active,
        registered_by: AgentName::from_validated(raw.registered_by),
        registered_at: parse_stamp(&key, &raw.registered_at)?,
        disabled_at: parse_optional_stamp(&key, raw.disabled_at.as_deref())?,
    })
}

fn read_rows(
    db: &Arc<SharedDb>,
    connection: &Connection,
    sql: &str,
    values: &[Value],
) -> Result<Vec<HookRegistration>, AtmError> {
    let raws = query_rows(db, connection, TABLE, sql, values, read_raw)?;
    raws.into_iter().map(decode).collect()
}

impl HookRegistrationStore for SqliteHookRegistrationStore {
    fn register(&self, registration: HookRegistration) -> Result<HookRegistration, AtmError> {
        let team = registration.team.as_str().to_owned();
        let phase = registration.phase.as_str().to_owned();
        let hook = registration.hook.as_str().to_owned();
        let module_path = registration.module_path.to_string_lossy().into_owned();
        let cwd = registration.cwd.to_string_lossy().into_owned();
        let timeout_ms = i64::from(registration.timeout_ms);
        let custom = encode_custom(registration.custom);
        let registered_by = registration.registered_by.as_str().to_owned();
        let registered_at = stamp(registration.registered_at);
        self.db.with_transaction(|transaction| {
            // An active key keeps its stored `registered_at` (chain position);
            // an inactive key takes the argument and becomes active again.
            transaction
                .execute(
                    "INSERT INTO hook_registration
                        (team, phase, hook, module_path, cwd, timeout_ms, custom,
                         active, registered_by, registered_at, disabled_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, NULL)
                     ON CONFLICT(team, phase, hook) DO UPDATE SET
                        module_path = excluded.module_path,
                        cwd = excluded.cwd,
                        timeout_ms = excluded.timeout_ms,
                        custom = excluded.custom,
                        registered_by = excluded.registered_by,
                        registered_at = CASE WHEN hook_registration.active = 1
                            THEN hook_registration.registered_at
                            ELSE excluded.registered_at END,
                        active = 1,
                        disabled_at = NULL;",
                    params![
                        team,
                        phase,
                        hook,
                        module_path,
                        cwd,
                        timeout_ms,
                        custom,
                        registered_by,
                        registered_at
                    ],
                )
                .map_err(|error| write_error(&self.db, TABLE, error))?;
            let mut rows = read_rows(
                &self.db,
                transaction,
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM hook_registration
                     WHERE team = ?1 AND phase = ?2 AND hook = ?3;"
                ),
                &[text(&team), text(&phase), text(&hook)],
            )?;
            rows.pop().ok_or_else(|| {
                internal(
                    &format!("{TABLE} {team}/{phase}/{hook}"),
                    "row missing after upsert",
                )
            })
        })
    }

    fn disable(
        &self,
        team: &TeamName,
        phase: &PhaseId,
        hook: Option<&HookName>,
        at: IsoTimestamp,
    ) -> Result<Vec<HookRegistration>, AtmError> {
        let hook_filter = hook.map(|hook| hook.as_str().to_owned());
        let filter_values = [
            text(team.as_str()),
            text(phase.as_str()),
            hook_filter.as_deref().map_or(Value::Null, text),
        ];
        let at_text = stamp(at);
        self.db.with_transaction(|transaction| {
            let selected = read_rows(
                &self.db,
                transaction,
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM hook_registration
                     WHERE team = ?1 AND phase = ?2 AND active = 1
                       AND (?3 IS NULL OR hook = ?3)
                     ORDER BY registered_at, hook;"
                ),
                &filter_values,
            )?;
            transaction
                .execute(
                    "UPDATE hook_registration SET active = 0, disabled_at = ?4
                     WHERE team = ?1 AND phase = ?2 AND active = 1
                       AND (?3 IS NULL OR hook = ?3);",
                    params![team.as_str(), phase.as_str(), hook_filter, at_text],
                )
                .map_err(|error| write_error(&self.db, TABLE, error))?;
            Ok(selected
                .into_iter()
                .map(|mut row| {
                    row.active = false;
                    row.disabled_at = Some(at);
                    row
                })
                .collect())
        })
    }

    fn active(&self, team: &TeamName, hook: &HookName) -> Result<Vec<HookRegistration>, AtmError> {
        let values = [text(team.as_str()), text(hook.as_str())];
        let db = Arc::clone(&self.db);
        self.db.read(move |connection| {
            read_rows(
                &db,
                connection,
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM hook_registration
                     WHERE team = ?1 AND hook = ?2 AND active = 1
                     ORDER BY registered_at, phase;"
                ),
                &values,
            )
        })
    }

    fn list(
        &self,
        team: &TeamName,
        phase: Option<&PhaseId>,
    ) -> Result<Vec<HookRegistration>, AtmError> {
        let values = [
            text(team.as_str()),
            phase.map_or(Value::Null, |phase| text(phase.as_str())),
        ];
        let db = Arc::clone(&self.db);
        self.db.read(move |connection| {
            read_rows(
                &db,
                connection,
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM hook_registration
                     WHERE team = ?1 AND (?2 IS NULL OR phase = ?2)
                     ORDER BY active DESC, registered_at, phase;"
                ),
                &values,
            )
        })
    }

    fn active_all(&self) -> Result<Vec<HookRegistration>, AtmError> {
        let db = Arc::clone(&self.db);
        self.db.read(move |connection| {
            read_rows(
                &db,
                connection,
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM hook_registration
                     WHERE active = 1
                     ORDER BY team, hook, registered_at, phase;"
                ),
                &[],
            )
        })
    }
}
