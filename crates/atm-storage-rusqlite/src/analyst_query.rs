//! SQLite implementation of the deliberately local analyst-query capability.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use atm_storage::{AnalystQueryRow, AnalystQueryStore, AnalystQueryValue, AtmError};
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::{Connection, OpenFlags, params_from_iter, types::Value};

pub fn open_analyst_query_store(
    path: impl AsRef<Path>,
) -> Result<Box<dyn AnalystQueryStore>, AtmError> {
    Ok(Box::new(SqliteAnalystQueryStore {
        path: path.as_ref().to_owned(),
    }))
}

/// Creates a deliberately minimal durable analyst-view fixture.
///
/// This test-support helper keeps all SQLite setup inside the SQLite backend;
/// consumers such as the Python extension never import `rusqlite` directly.
#[cfg(feature = "test-support")]
pub fn create_analyst_query_fixture_for_test(path: impl AsRef<Path>) -> Result<(), AtmError> {
    let connection = Connection::open(path.as_ref()).map_err(sql_error)?;
    connection
        .execute_batch(
            "CREATE TABLE hidden_mail_messages (
                 team TEXT NOT NULL,
                 agent TEXT NOT NULL,
                 document_type TEXT NOT NULL,
                 phase TEXT NOT NULL,
                 value TEXT NOT NULL
             );
             CREATE VIEW decomposed_messages AS
                SELECT team, agent, document_type, phase, value FROM hidden_mail_messages;
             INSERT INTO hidden_mail_messages VALUES
                ('atm-dev', 'dev-one', 'assignment', 'an', 'first assignment'),
                ('atm-dev', 'dev-two', 'assignment', 'an', 'second assignment'),
                ('atm-dev', 'qa-one', 'qa-finding', 'jj', 'finding'),
                ('atm-dev', 'fix-one', 'fix-assignment', 'jj', 'fix');",
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
        AtmError::mailbox_read(format!("analyst query could not open database: {error}"))
    })?;
    connection
        .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA defensive=ON;")
        .map_err(|error| {
            AtmError::mailbox_read(format!(
                "analyst query could not configure read-only connection: {error}"
            ))
        })?;
    connection.authorizer(Some(authorizer));
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
        return Err(AtmError::validation(
            "ATM analyst queries must be one SQLite read-only statement",
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
            return Err(AtmError::validation(
                "ATM analyst query exceeded row budget",
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
                return Err(AtmError::validation(
                    "ATM analyst query exceeded result-byte budget",
                ));
            }
            Ok((name.clone(), value))
        })
        .collect()
}

fn sql_error(error: rusqlite::Error) -> AtmError {
    AtmError::mailbox_read(format!("ATM analyst query failed: {error}"))
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
        return Err(AtmError::validation(
            "ATM analyst query has an unterminated SQL block comment",
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
    AtmError::validation("ATM analyst queries must contain exactly one statement")
}
