//! SQLite implementation of the deliberately local analyst-query capability.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(test)]
use crate::shared_db::{SharedDbTarget, record_opened_connection};
use atm_storage::{AnalystQueryRow, AnalystQueryStore, AnalystQueryValue, AtmError};
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::{Connection, OpenFlags, params_from_iter, types::Value};

#[cfg(feature = "test-support")]
use atm_storage::roles::{ROLE_QUALITY_MANAGER, ROLE_TEAM_LEAD, TEAM_ATM_DEV};

#[cfg(feature = "test-support")]
const FIXTURE_TEAM: &str = TEAM_ATM_DEV;
#[cfg(feature = "test-support")]
const FIXTURE_TEAM_LEAD: &str = ROLE_TEAM_LEAD;
#[cfg(feature = "test-support")]
const FIXTURE_QUALITY_MANAGER: &str = ROLE_QUALITY_MANAGER;

#[cfg(feature = "test-support")]
fn fixture_sql(sql: &str) -> String {
    sql.replace("$FIXTURE_TEAM", FIXTURE_TEAM)
        .replace("$FIXTURE_TEAM_LEAD", FIXTURE_TEAM_LEAD)
        .replace("$FIXTURE_QUALITY_MANAGER", FIXTURE_QUALITY_MANAGER)
}

pub fn open_analyst_query_store(
    path: impl AsRef<Path>,
) -> Result<Box<dyn AnalystQueryStore>, AtmError> {
    Ok(Box::new(SqliteAnalystQueryStore {
        path: path.as_ref().to_owned(),
    }))
}

/// Creates a durable analyst-view fixture for raw-query consumers.
///
/// This test-support helper keeps all SQLite setup inside the SQLite backend;
/// consumers such as the Python extension never import `rusqlite` directly.
#[cfg(feature = "test-support")]
pub fn create_analyst_query_fixture_for_test(path: impl AsRef<Path>) -> Result<(), AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(sql_error)?;
    connection
        .execute_batch(&fixture_sql(
            r#"CREATE TABLE hidden_mail_messages (
                 team TEXT NOT NULL,
                 agent TEXT NOT NULL,
                 document_type TEXT NOT NULL,
                 phase TEXT NOT NULL,
                 value TEXT NOT NULL,
                 from_agent TEXT NOT NULL DEFAULT '',
                 message_at TEXT NOT NULL DEFAULT '',
                 message_id TEXT NOT NULL DEFAULT '',
                 template_sha TEXT NOT NULL DEFAULT '',
                 template_type TEXT NOT NULL DEFAULT '',
                 vars_json TEXT NOT NULL DEFAULT '{}',
                 category TEXT NOT NULL DEFAULT '',
                 tags_json TEXT NOT NULL DEFAULT '[]',
                 summary TEXT NOT NULL DEFAULT ''
             );
             CREATE VIEW decomposed_messages AS
                SELECT team, agent, document_type, phase, value, from_agent,
                       message_at, message_id, template_sha, template_type,
                       vars_json, category, tags_json, tags_json AS instance_tags_json,
                       '[]' AS applied_template_tags_json,
                       '[]' AS derived_tags_json,
                       '[]' AS effective_tags_json, summary
                  FROM hidden_mail_messages;
             INSERT INTO hidden_mail_messages (team, agent, document_type, phase, value) VALUES
                ('$FIXTURE_TEAM', 'dev-one', 'assignment', 'an', 'first assignment'),
                ('$FIXTURE_TEAM', 'dev-two', 'assignment', 'an', 'second assignment'),
                ('$FIXTURE_TEAM', 'qa-one', 'qa-finding', 'jj', 'finding'),
                ('$FIXTURE_TEAM', 'fix-one', 'fix-assignment', 'jj', 'fix'),
                ('fixture-team', 'fixture-one', 'assignment', 'an', 'fixture assignment');"#,
        ))
        .map_err(sql_error)
}

/// Adds the AN.8 corpus to the ordinary analyst fixture.
///
/// It holds real-template-shaped values alongside a deliberately unrelated
/// vocabulary, while leaving the generic fixture stable for other consumers.
#[cfg(feature = "test-support")]
pub fn create_an8_analyst_query_fixture_for_test(path: impl AsRef<Path>) -> Result<(), AtmError> {
    create_analyst_query_fixture_for_test(path.as_ref())?;
    let connection = Connection::open(path.as_ref()).map_err(sql_error)?;
    connection
        .execute_batch(&fixture_sql(
            r#"INSERT INTO hidden_mail_messages VALUES
                ('fixture-an8', 'dev-alpha', 'task', 'AN', 'AN.1 assignment',
                 '$FIXTURE_TEAM_LEAD', '2026-08-01T08:00:00Z', 'an8-001', 'a', 'dev-task',
                 '{"sprint":"AN.1","state":"assigned"}',
                 'assignment', '[]', 'assign AN.1'),
                ('fixture-an8', 'dev-alpha', 'task', 'AN', 'AN.1 completion',
                 'dev-alpha', '2026-08-01T10:00:00Z', 'an8-002', 'a', 'dev-task',
                 '{"sprint":"AN.1","state":"complete","developer":"dev-alpha"}',
                 'completion', '[]', 'complete AN.1'),
                ('fixture-an8', 'qa-alpha', 'qa', 'AN', 'AN.1 QA round one blocking',
                 '$FIXTURE_QUALITY_MANAGER', '2026-08-01T09:00:00Z', 'an8-003', 'b', 'qa-task',
                 '{"sprint":"AN.1","round":1,"severity":"Blocking"}',
                 'qa-finding', '[]', 'AN.1 blocking'),
                ('fixture-an8', 'qa-alpha', 'qa', 'AN', 'AN.1 QA round one minor',
                 '$FIXTURE_QUALITY_MANAGER', '2026-08-01T09:01:00Z', 'an8-004', 'b', 'qa-task',
                 '{"sprint":"AN.1","round":1,"severity":"Minor"}',
                 'qa-finding', '[]', 'AN.1 minor'),
                ('fixture-an8', 'qa-alpha', 'qa', 'AN', 'AN.1 QA round two important',
                 '$FIXTURE_QUALITY_MANAGER', '2026-08-01T09:30:00Z', 'an8-005', 'b', 'qa-task',
                 '{"sprint":"AN.1","round":2,"severity":"Important"}',
                 'qa-finding', '[]', 'AN.1 important'),
                ('fixture-an8', 'dev-beta', 'task', 'AN', 'AN.2 assignment',
                 '$FIXTURE_TEAM_LEAD', '2026-08-02T08:00:00Z', 'an8-006', 'a', 'dev-task',
                 '{"sprint":"AN.2","state":"assigned"}',
                 'assignment', '[]', 'assign AN.2'),
                ('fixture-an8', 'dev-beta', 'task', 'AN', 'AN.2 completion',
                 'dev-beta', '2026-08-02T11:00:00Z', 'an8-007', 'a', 'dev-task',
                 '{"sprint":"AN.2","state":"complete","developer":"dev-beta"}',
                 'completion', '[]', 'complete AN.2'),
                ('fixture-an8', 'qa-beta', 'qa', 'AN', 'AN.2 QA round one blocking',
                 '$FIXTURE_QUALITY_MANAGER', '2026-08-02T10:00:00Z', 'an8-008', 'b', 'qa-task',
                 '{"sprint":"AN.2","round":1,"severity":"Blocking"}',
                 'qa-finding', '[]', 'AN.2 blocking'),
                ('fixture-synthetic', 'owner-rose', 'work-item', 'PX', 'PX-7 opened',
                 'coordinator', '2026-08-03T08:00:00Z', 'synthetic-001', 'c', 'work-item',
                 '{"cycle":"PX-7","event":"opened","owner":"owner-rose"}',
                 'opened', '[]', 'open PX-7'),
                ('fixture-synthetic', 'reviewer-sage', 'assessment', 'PX', 'PX-7 risk high',
                 'reviewer-sage', '2026-08-03T09:00:00Z', 'synthetic-002', 'd', 'assessment',
                 '{"cycle":"PX-7","pass":1,"risk":"high"}',
                 'assessment', '[]', 'high risk'),
                ('fixture-synthetic', 'owner-rose', 'work-item', 'PX', 'PX-7 delivered',
                 'owner-rose', '2026-08-03T10:00:00Z', 'synthetic-003', 'c', 'work-item',
                 '{"cycle":"PX-7","event":"delivered","owner":"owner-rose"}',
                 'delivered', '[]', 'deliver PX-7');"#,
        ))
        .map_err(sql_error)
}

/// Creates the retained AN.12 workflow-view fixture for Python boundary tests.
///
/// The fixture intentionally uses two unrelated workflow vocabularies.  It
/// lives in the SQLite test-support crate so the Python extension continues to
/// exercise only its read-only query capability rather than importing SQLite.
#[cfg(feature = "test-support")]
pub fn create_an12_workflow_query_fixture_for_test(path: impl AsRef<Path>) -> Result<(), AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(sql_error)?;
    connection
        .execute_batch(
            r#"CREATE TABLE hidden_workflow_messages (
                   scope_kind TEXT NOT NULL,
                   scope_id TEXT NOT NULL,
                   workflow_state TEXT NOT NULL,
                   workflow_stage TEXT NOT NULL,
                   workflow_transition TEXT NOT NULL,
                   workflow_iteration TEXT NULL,
                   applied_template_tags_json TEXT NOT NULL,
                   effective_tags_json TEXT NOT NULL,
                   message_at TEXT NOT NULL
               );
               CREATE VIEW decomposed_messages AS
                 SELECT scope_kind AS workflow_scope_kind,
                        scope_id AS workflow_scope_id,
                        workflow_state,
                        workflow_stage,
                        workflow_transition,
                        workflow_iteration,
                        applied_template_tags_json,
                        effective_tags_json,
                        message_at
                   FROM hidden_workflow_messages;
               INSERT INTO hidden_workflow_messages VALUES
                 ('release-train', 'train-42', 'queued', 'prepare', 'enter', '1',
                  '["audience:operators","domain:delivery"]',
                  '["audience:operators","channel:release","content-format:xml","domain:delivery","template-type:notice","workflow-scope-kind:release-train","workflow-stage:prepare","workflow-state:queued","workflow-transition:enter"]',
                  '2026-08-10T09:00:00Z'),
                 ('release-train', 'train-42', 'shipped', 'release', 'exit', '1',
                  '["audience:operators","domain:delivery"]',
                  '["audience:operators","channel:release","content-format:xml","domain:delivery","template-type:notice","workflow-scope-kind:release-train","workflow-stage:release","workflow-state:shipped","workflow-transition:exit"]',
                  '2026-08-10T09:07:00Z'),
                 ('operation', 'north-pier', 'mobilized', 'dispatch', 'begin', NULL,
                  '["domain:field","retention:brief"]',
                  '["channel:dispatch","content-format:markdown","domain:field","retention:brief","template-type:dispatch-note","workflow-scope-kind:operation","workflow-stage:dispatch","workflow-state:mobilized","workflow-transition:begin"]',
                  '2026-08-11T14:00:00Z');"#,
        )
        .map_err(sql_error)
}

struct SqliteAnalystQueryStore {
    path: PathBuf,
}

impl AnalystQueryStore for SqliteAnalystQueryStore {
    fn query(
        &self,
        sql: &str,
        parameters: &[AnalystQueryValue],
        deadline: Duration,
        max_rows: usize,
        max_bytes: usize,
    ) -> Result<Vec<AnalystQueryRow>, AtmError> {
        reject_executable_tail(sql)?;
        let connection = open_defensive_connection(&self.path)?;
        install_query_budget(&connection, deadline);
        execute_readonly_query(&connection, sql, parameters, max_rows, max_bytes)
    }
}

fn open_defensive_connection(path: &Path) -> Result<Connection, AtmError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| {
        AtmError::mailbox_read("analyst query could not open database").with_cause(error)
    })?;
    connection
        .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA defensive=ON;")
        .map_err(|error| {
            AtmError::mailbox_read("analyst query could not configure read-only connection")
                .with_cause(error)
        })?;
    connection.authorizer(Some(authorizer));
    #[cfg(test)]
    record_opened_connection(&SharedDbTarget::Path(path.to_path_buf()));
    Ok(connection)
}

fn install_query_budget(connection: &Connection, deadline: Duration) {
    let started = Instant::now();
    connection.progress_handler(1_000, Some(move || started.elapsed() > deadline));
}

fn execute_readonly_query(
    connection: &Connection,
    sql: &str,
    parameters: &[AnalystQueryValue],
    max_rows: usize,
    max_bytes: usize,
) -> Result<Vec<AnalystQueryRow>, AtmError> {
    let mut statement = connection.prepare(sql).map_err(sql_error)?;
    if !statement.readonly() {
        return Err(AtmError::validation_with_recovery(
            "ATM analyst queries must be one SQLite read-only statement",
            "submit exactly one SQLite SELECT statement",
        ));
    }
    let names = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let values = parameters.iter().map(to_sql_value).collect::<Vec<_>>();
    let rows = statement
        .query(params_from_iter(values))
        .map_err(sql_error)?;
    collect_query_rows(rows, &names, max_rows, max_bytes)
}

fn collect_query_rows(
    mut rows: rusqlite::Rows<'_>,
    names: &[String],
    max_rows: usize,
    max_bytes: usize,
) -> Result<Vec<AnalystQueryRow>, AtmError> {
    let mut output = Vec::new();
    let mut bytes = 0_usize;
    while let Some(row) = rows.next().map_err(sql_error)? {
        if output.len() >= max_rows {
            return Err(AtmError::validation_with_recovery(
                "ATM analyst query exceeded row budget",
                "narrow the query or request fewer rows",
            ));
        }
        output.push(collect_one_row(row, names, &mut bytes, max_bytes)?);
    }
    Ok(output)
}

fn collect_one_row(
    row: &rusqlite::Row<'_>,
    names: &[String],
    bytes: &mut usize,
    max_bytes: usize,
) -> Result<AnalystQueryRow, AtmError> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let value = from_sql_value(row.get(index).map_err(sql_error)?);
            *bytes += value_bytes(&value);
            if *bytes > max_bytes {
                return Err(AtmError::validation_with_recovery(
                    "ATM analyst query exceeded result-byte budget",
                    "narrow the selected columns or reduce the result set",
                ));
            }
            Ok((name.clone(), value))
        })
        .collect()
}

fn sql_error(error: rusqlite::Error) -> AtmError {
    AtmError::mailbox_read("ATM analyst query failed").with_cause(error)
}

fn to_sql_value(value: &AnalystQueryValue) -> Value {
    match value {
        AnalystQueryValue::Null => Value::Null,
        AnalystQueryValue::Integer(value) => Value::Integer(*value),
        AnalystQueryValue::Real(value) => Value::Real(*value),
        AnalystQueryValue::Text(value) => Value::Text(value.clone()),
        AnalystQueryValue::Blob(value) => Value::Blob(value.clone()),
    }
}

fn from_sql_value(value: Value) -> AnalystQueryValue {
    match value {
        Value::Null => AnalystQueryValue::Null,
        Value::Integer(value) => AnalystQueryValue::Integer(value),
        Value::Real(value) => AnalystQueryValue::Real(value),
        Value::Text(value) => AnalystQueryValue::Text(value),
        Value::Blob(value) => AnalystQueryValue::Blob(value),
    }
}

fn value_bytes(value: &AnalystQueryValue) -> usize {
    match value {
        AnalystQueryValue::Text(value) => value.len(),
        AnalystQueryValue::Blob(value) => value.len(),
        _ => 0,
    }
}

fn authorizer(context: AuthContext<'_>) -> Authorization {
    match context.action {
        AuthAction::Read { table_name, .. }
            if context.accessor == Some("decomposed_messages")
                || matches!(
                    table_name,
                    "decomposed_messages" | "sqlite_master" | "sqlite_schema"
                ) =>
        {
            Authorization::Allow
        }
        AuthAction::Select | AuthAction::Recursive => Authorization::Allow,
        AuthAction::Function { function_name } if function_name != "load_extension" => {
            Authorization::Allow
        }
        AuthAction::Pragma {
            pragma_name: "table_info" | "table_xinfo",
            pragma_value: None,
        } => Authorization::Allow,
        _ => Authorization::Deny,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SqlState {
    Normal,
    SingleQuote,
    DoubleQuote,
    Backtick,
    Bracket,
    LineComment,
    BlockComment,
}

fn reject_executable_tail(sql: &str) -> Result<(), AtmError> {
    let mut state = SqlState::Normal;
    let mut ended = false;
    let bytes = sql.as_bytes();
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        index += advance_sql_scanner(byte, bytes.get(index + 1).copied(), &mut state, &mut ended)?;
        index += 1;
    }
    if state == SqlState::BlockComment {
        return Err(AtmError::validation_with_recovery(
            "ATM analyst query has an unterminated SQL block comment",
            "close the SQL block comment before submitting",
        ));
    }
    Ok(())
}

fn advance_sql_scanner(
    byte: u8,
    next: Option<u8>,
    state: &mut SqlState,
    ended: &mut bool,
) -> Result<usize, AtmError> {
    if *state != SqlState::Normal {
        return advance_nested_sql_state(byte, next, state);
    }
    match byte {
        b'\'' => enter_sql_state(SqlState::SingleQuote, *ended, state),
        b'"' => enter_sql_state(SqlState::DoubleQuote, *ended, state),
        b'`' => enter_sql_state(SqlState::Backtick, *ended, state),
        b'[' => enter_sql_state(SqlState::Bracket, *ended, state),
        b'-' if next == Some(b'-') => {
            *state = SqlState::LineComment;
            Ok(1)
        }
        b'/' if next == Some(b'*') => {
            *state = SqlState::BlockComment;
            Ok(1)
        }
        b';' => {
            *ended = true;
            Ok(0)
        }
        value if value.is_ascii_whitespace() => Ok(0),
        _ if *ended => Err(single_statement_error()),
        _ => Ok(0),
    }
}

fn enter_sql_state(next: SqlState, ended: bool, state: &mut SqlState) -> Result<usize, AtmError> {
    if ended {
        return Err(single_statement_error());
    }
    *state = next;
    Ok(0)
}

fn advance_nested_sql_state(
    byte: u8,
    next: Option<u8>,
    state: &mut SqlState,
) -> Result<usize, AtmError> {
    match (*state, byte, next) {
        (SqlState::SingleQuote, b'\'', Some(b'\'')) | (SqlState::DoubleQuote, b'"', Some(b'"')) => {
            Ok(1)
        }
        (SqlState::SingleQuote, b'\'', _)
        | (SqlState::DoubleQuote, b'"', _)
        | (SqlState::Backtick, b'`', _)
        | (SqlState::Bracket, b']', _) => {
            *state = SqlState::Normal;
            Ok(0)
        }
        (SqlState::LineComment, b'\n', _) | (SqlState::BlockComment, b'*', Some(b'/')) => {
            *state = SqlState::Normal;
            Ok(1)
        }
        _ => Ok(0),
    }
}

fn single_statement_error() -> AtmError {
    AtmError::validation_with_recovery(
        "ATM analyst queries must contain exactly one statement",
        "submit exactly one SQLite SELECT statement",
    )
}

#[cfg(test)]
mod tests {
    use super::{open_defensive_connection, sql_error};
    use crate::observability::NullSqliteObservability;
    use crate::reader_pool::{DEFAULT_DOCTOR_READER_CONFIG, ReaderLanesConfig, ReaderPool};
    use crate::shared_db::{
        SharedDbTarget, opened_connection_count, reset_opened_connection_count,
    };
    use crate::shared_db_reader_lanes::SharedDb;
    use std::sync::Arc;

    #[test]
    fn sqlite_errors_preserve_the_adapter_cause() {
        let error = sql_error(rusqlite::Error::InvalidQuery);
        assert!(error.message().starts_with("ATM analyst query failed"));
        assert_eq!(error.cause(), Some("Query is not read-only"));
    }

    #[test]
    fn default_reader_lane_composition_opens_the_documented_connection_budget() {
        let root = tempfile::tempdir().expect("temporary SQLite root");
        let path = root.path().join("mail.db");
        let target = SharedDbTarget::Path(path.clone());
        reset_opened_connection_count(&target);

        let _database = SharedDb::open_with_reader_lanes(
            &path,
            Arc::new(NullSqliteObservability),
            ReaderLanesConfig::default(),
        )
        .expect("default writer, mailbox, and search lanes");
        let _doctor = ReaderPool::start(
            "doctor",
            Arc::new(target.clone()),
            DEFAULT_DOCTOR_READER_CONFIG,
        )
        .expect("default doctor lane");
        let _analyst = open_defensive_connection(&path).expect("analyst read connection");

        let opened = opened_connection_count(&target);
        assert_eq!(opened, 12, "writer + mailbox + search + doctor + analyst");
        assert!(
            opened <= 22,
            "opened connections must fit the worst-case cap"
        );
    }
}
