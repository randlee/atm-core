//! Local-only, defensive Python analyst interface over `decomposed_messages`.
//!
//! This is deliberately not an ATM client: it opens a selected SQLite file
//! read-only and exposes one parameterized statement at a time.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::{Connection, OpenFlags, params_from_iter, types::Value};

const MAX_ROWS: usize = 10_000;
const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;
const QUERY_BUDGET: Duration = Duration::from_secs(3);

#[pyclass]
struct ReadonlyDatabase {
    path: PathBuf,
}

#[pymethods]
impl ReadonlyDatabase {
    fn query(
        &self,
        py: Python<'_>,
        sql: &str,
        parameters: Option<Bound<'_, PyTuple>>,
    ) -> PyResult<Py<PyAny>> {
        let connection = open_connection(&self.path)?;
        install_query_budget(&connection, Instant::now());
        reject_executable_tail(sql)?;
        let mut statement = connection.prepare(sql).map_err(sql_error)?;
        if !statement.readonly() {
            return Err(PyValueError::new_err(
                "ATM analyst queries must be one SQLite read-only statement",
            ));
        }
        let parameters = parameters
            .map(|values| {
                values
                    .iter()
                    .map(python_value)
                    .collect::<PyResult<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        let columns = statement
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut rows = statement
            .query(params_from_iter(parameters))
            .map_err(sql_error)?;
        let output = PyList::empty(py);
        let mut bytes = 0usize;
        while let Some(row) = rows.next().map_err(sql_error)? {
            if output.len() >= MAX_ROWS {
                return Err(PyRuntimeError::new_err(
                    "ATM analyst query exceeded row budget",
                ));
            }
            let mapping = PyDict::new(py);
            for (index, name) in columns.iter().enumerate() {
                let value: Value = row.get(index).map_err(sql_error)?;
                let python = value_to_python(py, value, &mut bytes)?;
                mapping.set_item(name, python)?;
            }
            output.append(mapping)?;
        }
        Ok(output.into_any().unbind())
    }
}

#[pyfunction]
#[pyo3(signature = (database_path=None))]
fn open_readonly(database_path: Option<String>) -> PyResult<ReadonlyDatabase> {
    let path = database_path
        .map(PathBuf::from)
        .unwrap_or_else(default_database_path);
    // Validate the connection and all defensive guards before returning it.
    let _ = open_connection(&path)?;
    Ok(ReadonlyDatabase { path })
}

fn default_database_path() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        // Durable ATM state is host-scoped. `ATM_HOME` configures workspace
        // discovery and must never select the analyst database implicitly.
        // [cass: helpful b-mr7coqmo-qft2s6] Preserve isolated host worktrees.
        .join(".atm")
        .join("db")
        .join("mail.db")
}

fn open_connection(path: &PathBuf) -> PyResult<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sql_error)?;
    connection
        .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA defensive=ON;")
        .map_err(sql_error)?;
    connection.authorizer(Some(authorizer));
    Ok(connection)
}

fn install_query_budget(connection: &Connection, started: Instant) {
    connection.progress_handler(1_000, Some(move || started.elapsed() > QUERY_BUDGET));
}

fn authorizer(context: AuthContext<'_>) -> Authorization {
    match context.action {
        // Restrict reads to the versioned analyst view and SQLite's metadata
        // tables. Underlying ATM tables intentionally remain private.
        AuthAction::Read { table_name, .. }
            if context.accessor == Some("decomposed_messages")
                || matches!(
                    table_name,
                    "decomposed_messages" | "sqlite_master" | "sqlite_schema"
                ) =>
        {
            Authorization::Allow
        }
        AuthAction::Select => Authorization::Allow,
        // Pure built-ins are required for ordinary projections and aggregates;
        // the one extension-loading function remains explicitly forbidden.
        AuthAction::Function { function_name } if function_name != "load_extension" => {
            Authorization::Allow
        }
        // Recursive CTEs remain read-only and are useful for analyst-side
        // aggregation; the progress handler still enforces the time budget.
        AuthAction::Recursive => Authorization::Allow,
        AuthAction::Pragma {
            pragma_name: "table_info" | "table_xinfo",
            pragma_value: None,
        } => Authorization::Allow,
        _ => Authorization::Deny,
    }
}

fn python_value(value: Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(value) = value.extract::<i64>() {
        return Ok(Value::Integer(value));
    }
    if let Ok(value) = value.extract::<f64>() {
        return Ok(Value::Real(value));
    }
    if let Ok(value) = value.extract::<String>() {
        return Ok(Value::Text(value));
    }
    if let Ok(value) = value.extract::<Vec<u8>>() {
        return Ok(Value::Blob(value));
    }
    Err(PyValueError::new_err(
        "ATM analyst parameters must be null, int, float, str, or bytes",
    ))
}

fn value_to_python(py: Python<'_>, value: Value, bytes: &mut usize) -> PyResult<Py<PyAny>> {
    let output = match value {
        Value::Null => py.None(),
        Value::Integer(value) => value.into_pyobject(py)?.into_any().unbind(),
        Value::Real(value) => value.into_pyobject(py)?.into_any().unbind(),
        Value::Text(value) => {
            *bytes += value.len();
            value.into_pyobject(py)?.into_any().unbind()
        }
        Value::Blob(value) => {
            *bytes += value.len();
            value.into_pyobject(py)?.into_any().unbind()
        }
    };
    if *bytes > MAX_RESULT_BYTES {
        return Err(PyRuntimeError::new_err(
            "ATM analyst query exceeded result-byte budget",
        ));
    }
    Ok(output)
}

fn sql_error(error: rusqlite::Error) -> PyErr {
    PyRuntimeError::new_err(format!("ATM analyst query failed: {error}"))
}

/// Reject a second executable statement before SQLite can prepare or execute
/// it. Semicolons inside quoted SQL literals are harmless; a terminal
/// semicolon followed only by whitespace is accepted.
fn reject_executable_tail(sql: &str) -> PyResult<()> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        SingleQuote,
        DoubleQuote,
        Backtick,
        Bracket,
        LineComment,
        BlockComment,
    }

    let bytes = sql.as_bytes();
    let mut index = 0;
    let mut state = State::Normal;
    let mut statement_ended = false;
    while let Some(&byte) = bytes.get(index) {
        match state {
            State::Normal => match byte {
                b'\'' => {
                    if statement_ended {
                        return Err(single_statement_error());
                    }
                    state = State::SingleQuote;
                }
                b'\"' => {
                    if statement_ended {
                        return Err(single_statement_error());
                    }
                    state = State::DoubleQuote;
                }
                b'`' => {
                    if statement_ended {
                        return Err(single_statement_error());
                    }
                    state = State::Backtick;
                }
                b'[' => {
                    if statement_ended {
                        return Err(single_statement_error());
                    }
                    state = State::Bracket;
                }
                b'-' if bytes.get(index + 1) == Some(&b'-') => {
                    state = State::LineComment;
                    index += 1;
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    state = State::BlockComment;
                    index += 1;
                }
                b';' => statement_ended = true,
                byte if byte.is_ascii_whitespace() => {}
                _ if statement_ended => return Err(single_statement_error()),
                _ => {}
            },
            State::SingleQuote if byte == b'\'' && bytes.get(index + 1) == Some(&b'\'') => {
                index += 1;
            }
            State::SingleQuote if byte == b'\'' => state = State::Normal,
            State::DoubleQuote if byte == b'\"' && bytes.get(index + 1) == Some(&b'\"') => {
                index += 1;
            }
            State::DoubleQuote if byte == b'\"' => state = State::Normal,
            State::Backtick if byte == b'`' && bytes.get(index + 1) == Some(&b'`') => index += 1,
            State::Backtick if byte == b'`' => state = State::Normal,
            State::Bracket if byte == b']' => state = State::Normal,
            State::LineComment if byte == b'\n' => state = State::Normal,
            State::BlockComment if byte == b'*' && bytes.get(index + 1) == Some(&b'/') => {
                state = State::Normal;
                index += 1;
            }
            _ => {}
        }
        index += 1;
    }
    if state == State::BlockComment {
        return Err(PyValueError::new_err(
            "ATM analyst query has an unterminated SQL block comment",
        ));
    }
    Ok(())
}

fn single_statement_error() -> PyErr {
    PyValueError::new_err("ATM analyst queries must contain exactly one statement")
}

#[pymodule]
fn atm_query(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<ReadonlyDatabase>()?;
    module.add_function(wrap_pyfunction!(open_readonly, module)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_ROWS, QUERY_BUDGET, ReadonlyDatabase, default_database_path, install_query_budget,
        open_connection, reject_executable_tail,
    };
    use pyo3::prelude::*;
    use pyo3::types::PyTuple;
    use rusqlite::Connection;
    use std::time::Duration;
    use tempfile::tempdir;

    fn fixture() -> std::path::PathBuf {
        let directory = tempdir().expect("temp directory").keep();
        let path = directory.join("mail.db");
        let connection = Connection::open(&path).expect("fixture database");
        connection
            .execute_batch(
                "CREATE TABLE hidden_mail_messages (
                team TEXT, agent TEXT, document_type TEXT, phase TEXT, value TEXT
             );
             CREATE VIEW decomposed_messages AS
                SELECT team, agent, document_type, phase, value FROM hidden_mail_messages;
             INSERT INTO hidden_mail_messages VALUES
                ('atm-dev', 'dev-one', 'assignment', 'an', 'first assignment'),
                ('atm-dev', 'dev-two', 'assignment', 'an', 'second assignment'),
                ('atm-dev', 'qa-one', 'qa-finding', 'jj', 'finding'),
                ('atm-dev', 'fix-one', 'fix-assignment', 'jj', 'fix');",
            )
            .expect("fixture schema");
        path
    }

    #[test]
    fn defensive_connection_allows_read_only_select_and_denies_writes() {
        let path = fixture();
        let connection = open_connection(&path).expect("connection");
        assert!(
            connection
                .prepare("SELECT * FROM decomposed_messages")
                .expect("select")
                .readonly()
        );
        assert!(
            connection
                .prepare("DELETE FROM decomposed_messages")
                .is_err()
        );
        assert!(
            connection
                .prepare("ATTACH DATABASE ':memory:' AS other")
                .is_err()
        );
        assert!(reject_executable_tail("SELECT 1; SELECT 2").is_err());
        assert!(reject_executable_tail("SELECT ';' AS literal;").is_ok());
        assert!(reject_executable_tail("SELECT 'it''s; still one statement'; -- note\n").is_ok());
        assert!(reject_executable_tail("SELECT 1; /* comment */ SELECT 2").is_err());
        assert!(
            connection
                .prepare("SELECT * FROM hidden_mail_messages")
                .is_err()
        );
        assert!(connection.prepare("PRAGMA database_list").is_err());
    }

    #[test]
    fn parameterized_query_returns_column_labelled_rows() {
        Python::initialize();
        let path = fixture();
        Python::attach(|py| {
            let database = ReadonlyDatabase { path };
            let parameters = PyTuple::new(py, ["atm-dev"]).expect("parameters");
            let result = database
                .query(
                    py,
                    "SELECT team, value FROM decomposed_messages WHERE team = ?",
                    Some(parameters),
                )
                .expect("query");
            let list = result
                .bind(py)
                .clone()
                .cast_into::<pyo3::types::PyList>()
                .expect("list");
            assert_eq!(list.len(), 4);
            assert_eq!(
                list.get_item(0)
                    .expect("row")
                    .get_item("value")
                    .expect("value")
                    .extract::<String>()
                    .expect("text"),
                "first assignment"
            );
        });
    }

    #[test]
    fn analyst_examples_use_parameterized_filters_over_the_stable_view() {
        Python::initialize();
        let path = fixture();
        Python::attach(|py| {
            let database = ReadonlyDatabase { path };
            let development_agents = PyTuple::new(py, ["dev-one", "dev-two"]).expect("agents");
            let assignments = database
                .query(
                    py,
                    "SELECT agent FROM decomposed_messages \
                     WHERE document_type = 'assignment' AND agent IN (?, ?) ORDER BY agent",
                    Some(development_agents),
                )
                .expect("development assignments");
            assert_eq!(
                assignments
                    .bind(py)
                    .clone()
                    .cast_into::<pyo3::types::PyList>()
                    .expect("rows")
                    .len(),
                2
            );

            let phase_parameters = PyTuple::new(py, ["an", "assignment"]).expect("phase");
            let phase_assignments = database
                .query(
                    py,
                    "SELECT value FROM decomposed_messages \
                     WHERE phase = ? AND document_type = ?",
                    Some(phase_parameters),
                )
                .expect("phase assignments");
            assert_eq!(
                phase_assignments
                    .bind(py)
                    .clone()
                    .cast_into::<pyo3::types::PyList>()
                    .expect("rows")
                    .len(),
                2
            );

            let finding_parameters = PyTuple::new(py, ["jj", "fix-assignment", "qa-finding"])
                .expect("finding parameters");
            let phase_findings = database
                .query(
                    py,
                    "SELECT document_type FROM decomposed_messages \
                     WHERE phase = ? AND document_type IN (?, ?) ORDER BY document_type",
                    Some(finding_parameters),
                )
                .expect("phase fixes and findings");
            assert_eq!(
                phase_findings
                    .bind(py)
                    .clone()
                    .cast_into::<pyo3::types::PyList>()
                    .expect("rows")
                    .len(),
                2
            );
        });
    }

    #[test]
    fn raw_query_security_gates_fail_before_execution() {
        Python::initialize();
        let path = fixture();
        Python::attach(|py| {
            let database = ReadonlyDatabase { path };
            for sql in [
                "DELETE FROM decomposed_messages",
                "CREATE TABLE pwned (value TEXT)",
                "BEGIN",
                "ATTACH DATABASE ':memory:' AS other",
                "SELECT 1; SELECT 2",
                "SELECT * FROM hidden_mail_messages",
            ] {
                assert!(database.query(py, sql, None).is_err(), "{sql}");
            }
        });
    }

    #[test]
    fn raw_query_row_and_byte_budgets_fail_closed() {
        Python::initialize();
        let path = fixture();
        let connection = Connection::open(&path).expect("fixture connection");
        for _ in 0..MAX_ROWS {
            connection
                .execute(
                    "INSERT INTO hidden_mail_messages VALUES \
                     ('atm-dev', 'row-budget', 'assignment', 'an', 'row-budget')",
                    [],
                )
                .expect("fixture row");
        }
        Python::attach(|py| {
            let database = ReadonlyDatabase { path };
            assert!(
                database
                    .query(py, "SELECT team, value FROM decomposed_messages", None)
                    .is_err()
            );
            // The byte gate is deterministic without allocating a huge fixture.
            assert!(database
                .query(
                    py,
                    "SELECT printf('%0*d', 8388609, 0) AS oversized FROM decomposed_messages LIMIT 1",
                    None,
                )
                .is_err());
        });
    }

    #[test]
    fn raw_query_deadline_interrupts_a_read_only_recursive_cte() {
        let path = fixture();
        let connection = open_connection(&path).expect("connection");
        let started = std::time::Instant::now();
        install_query_budget(&connection, started);
        let error = connection
            .query_row(
                "WITH RECURSIVE counter(value) AS (\
                   VALUES(1) UNION ALL SELECT value + 1 FROM counter\
                 ) SELECT sum(value) FROM counter",
                [],
                |_| Ok(()),
            )
            .expect_err("unbounded read-only CTE must be interrupted");
        assert!(
            error.to_string().contains("interrupted"),
            "expected SQLite progress-handler interruption, got {error}"
        );
        assert!(
            started.elapsed() < QUERY_BUDGET + Duration::from_secs(2),
            "query budget must bound the read-only statement"
        );
    }

    #[test]
    fn default_path_is_host_scoped_not_workspace_scoped() {
        let path = default_database_path();
        assert!(path.ends_with(".atm/db/mail.db"), "{path:?}");
    }
}
