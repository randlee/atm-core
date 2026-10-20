//! SQLite adapter for the cron schedule store (ADR-069 D1, D4).
//!
//! `start` runs as one writer transaction that checks for an active row and
//! then inserts or restarts in place, so the duplicate check and the write
//! cannot interleave with another writer. Run-state writes are guarded by
//! `id`, `generation` and `active`, so a run that outlives its slot never
//! writes the restarted row.

use std::path::PathBuf;
use std::sync::Arc;

use atm_storage::contract::sealed;
use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{
    AtmError, CronSchedule, CronScheduleId, CronScheduleStore, NewCronSchedule, PhaseId,
    StopCondition,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, params_from_iter};

use crate::shared_db::SharedDb;
use crate::store_support::{
    encode_custom, internal, parse_custom, parse_optional_stamp, parse_stamp, query_rows, stamp,
    text, write_error,
};

struct SqliteCronScheduleStore {
    db: Arc<SharedDb>,
}

pub(crate) fn cron_schedule_store(db: Arc<SharedDb>) -> Arc<dyn CronScheduleStore + Send + Sync> {
    Arc::new(SqliteCronScheduleStore { db })
}

impl sealed::Sealed for SqliteCronScheduleStore {}

const TABLE: &str = "cron_schedule";

const SELECT_COLUMNS: &str = "id, generation, team, phase, name, cron_expr, module_path, cwd,
    args, timeout_ms, jitter_s, custom, active, until_at, stop_hook, created_by, created_at,
    last_run_at, last_outcome, ended_at";

/// The schedule fields written by both INSERT and restart. Keep their SQL
/// order and value order in one place so adding a stored field cannot make the
/// two statements drift apart.
type ScheduleColumnValue = fn(&ScheduleColumns) -> Value;

const SCHEDULE_COLUMNS: &[(&str, ScheduleColumnValue)] = &[
    ("team", |columns| text(&columns.team)),
    ("phase", |columns| text(&columns.phase)),
    ("name", |columns| text(&columns.name)),
    ("cron_expr", |columns| text(&columns.cron_expr)),
    ("module_path", |columns| text(&columns.module_path)),
    ("cwd", |columns| text(&columns.cwd)),
    ("args", |columns| text(&columns.args)),
    ("timeout_ms", |columns| Value::Integer(columns.timeout_ms)),
    ("jitter_s", |columns| Value::Integer(columns.jitter_s)),
    ("custom", |columns| text(&columns.custom)),
    ("until_at", |columns| {
        columns.until_at.as_deref().map_or(Value::Null, text)
    }),
    ("stop_hook", |columns| Value::Integer(columns.stop_hook)),
    ("created_by", |columns| text(&columns.created_by)),
    ("created_at", |columns| text(&columns.created_at)),
];

fn numbered_placeholders(first: usize, count: usize) -> String {
    (first..first + count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One `cron_schedule` row as stored, before the typed fields are parsed.
struct RawCron {
    id: i64,
    generation: i64,
    team: String,
    phase: String,
    name: String,
    cron_expr: String,
    module_path: String,
    cwd: String,
    args: String,
    timeout_ms: i64,
    jitter_s: i64,
    custom: String,
    active: bool,
    until_at: Option<String>,
    stop_hook: bool,
    created_by: String,
    created_at: String,
    last_run_at: Option<String>,
    last_outcome: Option<String>,
    ended_at: Option<String>,
}

fn read_raw(row: &Row<'_>) -> rusqlite::Result<RawCron> {
    Ok(RawCron {
        id: row.get(0)?,
        generation: row.get(1)?,
        team: row.get(2)?,
        phase: row.get(3)?,
        name: row.get(4)?,
        cron_expr: row.get(5)?,
        module_path: row.get(6)?,
        cwd: row.get(7)?,
        args: row.get(8)?,
        timeout_ms: row.get(9)?,
        jitter_s: row.get(10)?,
        custom: row.get(11)?,
        active: row.get::<_, i64>(12)? == 1,
        until_at: row.get(13)?,
        stop_hook: row.get::<_, i64>(14)? == 1,
        created_by: row.get(15)?,
        created_at: row.get(16)?,
        last_run_at: row.get(17)?,
        last_outcome: row.get(18)?,
        ended_at: row.get(19)?,
    })
}

fn decode(raw: RawCron) -> Result<CronSchedule, AtmError> {
    let key = format!("{TABLE} id {}", raw.id);
    let stop = match (raw.stop_hook, raw.until_at.as_deref()) {
        (true, None) => StopCondition::Hook,
        (false, Some(until_at)) => StopCondition::Until {
            at: parse_stamp(&key, until_at)?.into_inner(),
        },
        _ => return Err(internal(&key, "stop condition is neither hook nor until")),
    };
    let args =
        serde_json::from_str::<Vec<String>>(&raw.args).map_err(|error| internal(&key, error))?;
    Ok(CronSchedule {
        id: CronScheduleId(u64::try_from(raw.id).map_err(|_| internal(&key, "id is negative"))?),
        generation: u32::try_from(raw.generation)
            .map_err(|_| internal(&key, "generation is out of range"))?,
        team: TeamName::from_validated(raw.team),
        phase: PhaseId::parse(&raw.phase)?,
        name: raw.name,
        cron_expr: raw.cron_expr,
        module_path: PathBuf::from(raw.module_path),
        cwd: PathBuf::from(raw.cwd),
        args,
        timeout_ms: u32::try_from(raw.timeout_ms)
            .map_err(|_| internal(&key, "timeout_ms is out of range"))?,
        jitter_s: u32::try_from(raw.jitter_s)
            .map_err(|_| internal(&key, "jitter_s is out of range"))?,
        custom: parse_custom(&key, &raw.custom)?,
        stop,
        active: raw.active,
        created_by: AgentName::from_validated(raw.created_by),
        created_at: parse_stamp(&key, &raw.created_at)?,
        last_run_at: parse_optional_stamp(&key, raw.last_run_at.as_deref())?,
        last_outcome: raw.last_outcome,
        ended_at: parse_optional_stamp(&key, raw.ended_at.as_deref())?,
    })
}

fn read_rows(
    db: &Arc<SharedDb>,
    connection: &Connection,
    sql: &str,
    values: &[Value],
) -> Result<Vec<CronSchedule>, AtmError> {
    let raws = query_rows(db, connection, TABLE, sql, values, read_raw)?;
    raws.into_iter().map(decode).collect()
}

fn list_query(
    team: Option<&TeamName>,
    phase: Option<&PhaseId>,
    active_only: bool,
) -> (String, [Value; 3]) {
    let values = [
        team.map_or(Value::Null, |team| text(team.as_str())),
        phase.map_or(Value::Null, |phase| text(phase.as_str())),
        Value::Integer(if active_only { 1 } else { 0 }),
    ];
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM cron_schedule
         WHERE (?1 IS NULL OR team = ?1) AND (?2 IS NULL OR phase = ?2)
           AND (?3 = 0 OR active = 1)
         ORDER BY active DESC, created_at, phase, name;"
    );
    (sql, values)
}

/// `(until_at, stop_hook)` for the stored stop columns: exactly one is set.
fn stop_columns(stop: &StopCondition) -> (Option<String>, i64) {
    match stop {
        StopCondition::Until { at } => (Some(stamp(IsoTimestamp::from(*at))), 0),
        StopCondition::Hook => (None, 1),
    }
}

/// Converts a schedule id to the stored integer. Ids above `i64::MAX` cannot
/// be stored, so no row can carry them.
fn stored_id(id: CronScheduleId) -> Option<i64> {
    i64::try_from(id.0).ok()
}

/// One schedule's column values, prepared before the writer transaction.
struct ScheduleColumns {
    team: String,
    phase: String,
    name: String,
    cron_expr: String,
    module_path: String,
    cwd: String,
    args: String,
    timeout_ms: i64,
    jitter_s: i64,
    custom: String,
    until_at: Option<String>,
    stop_hook: i64,
    created_by: String,
    created_at: String,
}

impl ScheduleColumns {
    fn new(schedule: NewCronSchedule) -> Result<Self, AtmError> {
        let (until_at, stop_hook) = stop_columns(&schedule.stop);
        Ok(Self {
            team: schedule.team.as_str().to_owned(),
            phase: schedule.phase.as_str().to_owned(),
            name: schedule.name,
            cron_expr: schedule.cron_expr,
            module_path: schedule.module_path.to_string_lossy().into_owned(),
            cwd: schedule.cwd.to_string_lossy().into_owned(),
            // `Vec<String>` is always serializable, and `to_string` writes to an
            // infallible `String`; this mapping is unreachable for this concrete type.
            args: serde_json::to_string(&schedule.args).map_err(|error| internal(TABLE, error))?,
            timeout_ms: i64::from(schedule.timeout_ms),
            jitter_s: i64::from(schedule.jitter_s),
            custom: encode_custom(schedule.custom),
            until_at,
            stop_hook,
            created_by: schedule.created_by.as_str().to_owned(),
            created_at: stamp(schedule.created_at),
        })
    }

    fn values(&self) -> Vec<Value> {
        SCHEDULE_COLUMNS
            .iter()
            .map(|(_, value)| value(self))
            .collect()
    }

    /// Inserts a new row at generation 1 and returns its id.
    fn insert(&self, transaction: &Transaction<'_>, db: &SharedDb) -> Result<i64, AtmError> {
        let columns = SCHEDULE_COLUMNS
            .iter()
            .map(|(column, _)| *column)
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = numbered_placeholders(1, SCHEDULE_COLUMNS.len());
        let sql = format!(
            "INSERT INTO cron_schedule (generation, {columns}, active)
             VALUES (1, {placeholders}, 1);"
        );
        let values = self.values();
        transaction
            .execute(&sql, params_from_iter(values.iter()))
            .map_err(|error| write_error(db, TABLE, error))?;
        Ok(transaction.last_insert_rowid())
    }

    /// Restarts an ended row in place: id kept, generation + 1, run state cleared.
    fn restart(
        &self,
        transaction: &Transaction<'_>,
        db: &SharedDb,
        id: i64,
    ) -> Result<(), AtmError> {
        let assignments = SCHEDULE_COLUMNS
            .iter()
            .enumerate()
            .map(|(index, (column, _))| format!("{column} = ?{}", index + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let id_placeholder = SCHEDULE_COLUMNS.len() + 1;
        let sql = format!(
            "UPDATE cron_schedule SET
                generation = generation + 1,
                {assignments},
                active = 1,
                last_run_at = NULL, last_outcome = NULL, ended_at = NULL
             WHERE id = ?{id_placeholder};"
        );
        let mut values = self.values();
        values.push(Value::Integer(id));
        transaction
            .execute(&sql, params_from_iter(values.iter()))
            .map_err(|error| write_error(db, TABLE, error))?;
        Ok(())
    }
}

impl CronScheduleStore for SqliteCronScheduleStore {
    fn start(&self, schedule: NewCronSchedule) -> Result<CronSchedule, AtmError> {
        let columns = ScheduleColumns::new(schedule)?;
        self.db.with_transaction(|transaction| {
            let existing = transaction
                .query_row(
                    "SELECT id, active FROM cron_schedule
                     WHERE team = ?1 AND phase = ?2 AND name = ?3;",
                    params![columns.team, columns.phase, columns.name],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? == 1)),
                )
                .optional()
                .map_err(|error| self.db.error("failed to read cron_schedule", error))?;
            let id = match existing {
                Some((_, true)) => {
                    let (name, phase) = (&columns.name, &columns.phase);
                    return Err(AtmError::validation_with_recovery(
                        format!("cron schedule {name} already active for phase {phase}"),
                        format!("run atm cron end --phase {phase} {name} first"),
                    ));
                }
                Some((id, false)) => {
                    columns.restart(transaction, &self.db, id)?;
                    id
                }
                None => columns.insert(transaction, &self.db)?,
            };
            let mut rows = read_rows(
                &self.db,
                transaction,
                &format!("SELECT {SELECT_COLUMNS} FROM cron_schedule WHERE id = ?1;"),
                &[Value::Integer(id)],
            )?;
            rows.pop()
                .ok_or_else(|| internal(&format!("{TABLE} id {id}"), "row missing after start"))
        })
    }

    fn end(
        &self,
        team: &TeamName,
        phase: &PhaseId,
        name: Option<&str>,
        at: IsoTimestamp,
    ) -> Result<Vec<CronSchedule>, AtmError> {
        let name_filter = name.map(str::to_owned);
        let filter_values = [
            text(team.as_str()),
            text(phase.as_str()),
            name_filter.as_deref().map_or(Value::Null, text),
        ];
        let at_text = stamp(at);
        self.db.with_transaction(|transaction| {
            let selected = read_rows(
                &self.db,
                transaction,
                &format!(
                    "SELECT {SELECT_COLUMNS} FROM cron_schedule
                     WHERE team = ?1 AND phase = ?2 AND active = 1
                       AND (?3 IS NULL OR name = ?3)
                     ORDER BY created_at, name;"
                ),
                &filter_values,
            )?;
            transaction
                .execute(
                    "UPDATE cron_schedule SET active = 0, ended_at = ?4
                     WHERE team = ?1 AND phase = ?2 AND active = 1
                       AND (?3 IS NULL OR name = ?3);",
                    params![team.as_str(), phase.as_str(), name_filter, at_text],
                )
                .map_err(|error| write_error(&self.db, TABLE, error))?;
            Ok(selected
                .into_iter()
                .map(|mut row| {
                    row.active = false;
                    row.ended_at = Some(at);
                    row
                })
                .collect())
        })
    }

    fn list(
        &self,
        team: Option<&TeamName>,
        phase: Option<&PhaseId>,
    ) -> Result<Vec<CronSchedule>, AtmError> {
        let (sql, values) = list_query(team, phase, false);
        let db = Arc::clone(&self.db);
        self.db
            .read(move |connection| read_rows(&db, connection, &sql, &values))
    }

    fn active_all(&self) -> Result<Vec<CronSchedule>, AtmError> {
        let (sql, values) = list_query(None, None, true);
        let db = Arc::clone(&self.db);
        self.db
            .read(move |connection| read_rows(&db, connection, &sql, &values))
    }

    fn record_run(
        &self,
        id: CronScheduleId,
        generation: u32,
        at: IsoTimestamp,
        outcome: &str,
    ) -> Result<bool, AtmError> {
        let Some(id) = stored_id(id) else {
            return Ok(false);
        };
        let at_text = stamp(at);
        let outcome = outcome.to_owned();
        self.db.with_transaction(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE cron_schedule SET last_run_at = ?3, last_outcome = ?4
                     WHERE id = ?1 AND generation = ?2 AND active = 1;",
                    params![id, i64::from(generation), at_text, outcome],
                )
                .map_err(|error| write_error(&self.db, TABLE, error))?;
            Ok(changed == 1)
        })
    }

    fn record_stop(
        &self,
        id: CronScheduleId,
        generation: u32,
        at: IsoTimestamp,
        reason: &str,
    ) -> Result<bool, AtmError> {
        let Some(id) = stored_id(id) else {
            return Ok(false);
        };
        let at_text = stamp(at);
        let outcome = format!("stopped: {reason}");
        self.db.with_transaction(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE cron_schedule SET active = 0, ended_at = ?3, last_outcome = ?4
                     WHERE id = ?1 AND generation = ?2 AND active = 1;",
                    params![id, i64::from(generation), at_text, outcome],
                )
                .map_err(|error| write_error(&self.db, TABLE, error))?;
            Ok(changed == 1)
        })
    }
}
