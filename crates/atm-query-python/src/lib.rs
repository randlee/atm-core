//! Local-only Python analyst interface over ATM's stable query view.
//!
//! The extension owns Python conversion and presentation only. SQLite access,
//! authorisation, and execution limits stay in `atm-storage-rusqlite`.

use std::path::PathBuf;
use std::time::Duration;

use atm_storage::{AnalystQueryStore, AnalystQueryValue};
use atm_storage_rusqlite::open_analyst_query_store;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

const MAX_ROWS: usize = 10_000;
const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;
const QUERY_BUDGET: Duration = Duration::from_secs(3);

#[pyclass]
struct ReadonlyDatabase {
    store: Box<dyn AnalystQueryStore>,
}

#[pymethods]
impl ReadonlyDatabase {
    fn query(
        &self,
        py: Python<'_>,
        sql: &str,
        parameters: Option<Bound<'_, PyTuple>>,
    ) -> PyResult<Py<PyAny>> {
        let parameters = python_parameters(parameters)?;
        let rows = self
            .store
            .query(sql, &parameters, QUERY_BUDGET, MAX_ROWS, MAX_RESULT_BYTES)
            .map_err(storage_error)?;
        rows_to_python(py, rows)
    }
}

#[pyfunction]
#[pyo3(signature = (database_path=None))]
fn open_readonly(database_path: Option<String>) -> PyResult<ReadonlyDatabase> {
    let path = database_path
        .map(PathBuf::from)
        .unwrap_or_else(default_database_path);
    Ok(ReadonlyDatabase {
        store: open_analyst_query_store(path).map_err(storage_error)?,
    })
}

fn default_database_path() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        // Durable ATM state is host-scoped. `ATM_HOME` configures workspace
        // discovery and must never select the analyst database implicitly.
        .join(".atm")
        .join("db")
        .join("mail.db")
}

fn python_parameters(parameters: Option<Bound<'_, PyTuple>>) -> PyResult<Vec<AnalystQueryValue>> {
    parameters
        .map(|values| values.iter().map(python_value).collect())
        .transpose()
        .map(|parameters| parameters.unwrap_or_default())
}

fn python_value(value: Bound<'_, PyAny>) -> PyResult<AnalystQueryValue> {
    if value.is_none() {
        return Ok(AnalystQueryValue::Null);
    }
    if let Ok(value) = value.extract::<i64>() {
        return Ok(AnalystQueryValue::Integer(value));
    }
    if let Ok(value) = value.extract::<f64>() {
        return Ok(AnalystQueryValue::Real(value));
    }
    if let Ok(value) = value.extract::<String>() {
        return Ok(AnalystQueryValue::Text(value));
    }
    if let Ok(value) = value.extract::<Vec<u8>>() {
        return Ok(AnalystQueryValue::Blob(value));
    }
    Err(PyValueError::new_err(
        "ATM analyst parameters must be null, int, float, str, or bytes",
    ))
}

fn rows_to_python(
    py: Python<'_>,
    rows: Vec<Vec<(String, AnalystQueryValue)>>,
) -> PyResult<Py<PyAny>> {
    let output = PyList::empty(py);
    for row in rows {
        let mapping = PyDict::new(py);
        for (name, value) in row {
            mapping.set_item(name, value_to_python(py, value)?)?;
        }
        output.append(mapping)?;
    }
    Ok(output.into_any().unbind())
}

fn value_to_python(py: Python<'_>, value: AnalystQueryValue) -> PyResult<Py<PyAny>> {
    match value {
        AnalystQueryValue::Null => Ok(py.None()),
        AnalystQueryValue::Integer(value) => Ok(value.into_pyobject(py)?.into_any().unbind()),
        AnalystQueryValue::Real(value) => Ok(value.into_pyobject(py)?.into_any().unbind()),
        AnalystQueryValue::Text(value) => Ok(value.into_pyobject(py)?.into_any().unbind()),
        AnalystQueryValue::Blob(value) => Ok(value.into_pyobject(py)?.into_any().unbind()),
    }
}

fn storage_error(error: atm_storage::AtmError) -> PyErr {
    PyRuntimeError::new_err(error.to_string())
}

#[pymodule]
fn atm_query(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<ReadonlyDatabase>()?;
    module.add_function(wrap_pyfunction!(open_readonly, module)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_ROWS, ReadonlyDatabase, default_database_path};
    use atm_storage_rusqlite::{create_analyst_query_fixture_for_test, open_analyst_query_store};
    use pyo3::prelude::*;
    use pyo3::types::{PyList, PyTuple};
    use tempfile::tempdir;

    const TEST_ANALYST_TEAM: &str = "atm-dev";

    fn fixture() -> std::path::PathBuf {
        let directory = tempdir().expect("temp directory").keep();
        let path = directory.join("mail.db");
        create_analyst_query_fixture_for_test(&path).expect("fixture database");
        path
    }

    fn database(path: std::path::PathBuf) -> ReadonlyDatabase {
        ReadonlyDatabase {
            store: open_analyst_query_store(path).expect("read-only store"),
        }
    }

    #[test]
    fn defensive_backend_allows_stable_view_and_denies_writes() {
        Python::initialize();
        Python::attach(|py| {
            let database = database(fixture());
            for sql in [
                "DELETE FROM decomposed_messages",
                "CREATE TABLE pwned (value TEXT)",
                "BEGIN",
                "ATTACH DATABASE ':memory:' AS other",
                "SELECT 1; SELECT 2",
                "SELECT * FROM hidden_mail_messages",
                "PRAGMA database_list",
            ] {
                assert!(database.query(py, sql, None).is_err(), "{sql}");
            }
            assert!(database.query(py, "SELECT ';' AS literal;", None).is_ok());
            assert!(
                database
                    .query(py, "SELECT 'it''s; still one statement'; -- note\n", None)
                    .is_ok()
            );
        });
    }

    #[test]
    fn parameterized_query_returns_column_labelled_rows() {
        Python::initialize();
        Python::attach(|py| {
            let database = database(fixture());
            let parameters = PyTuple::new(py, [TEST_ANALYST_TEAM]).expect("parameters");
            let result = database
                .query(
                    py,
                    "SELECT team, value FROM decomposed_messages WHERE team = ?",
                    Some(parameters),
                )
                .expect("query");
            let list = result.bind(py).clone().cast_into::<PyList>().expect("list");
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
    fn examples_use_parameterized_filters_over_the_stable_view() {
        Python::initialize();
        Python::attach(|py| {
            let database = database(fixture());
            let agents = PyTuple::new(py, ["dev-one", "dev-two"]).expect("agents");
            let assignments = database
                .query(
                    py,
                    "SELECT agent FROM decomposed_messages WHERE document_type = 'assignment' AND agent IN (?, ?) ORDER BY agent",
                    Some(agents),
                )
                .expect("assignments");
            assert_eq!(
                assignments.bind(py).cast::<PyList>().expect("rows").len(),
                2
            );
        });
    }

    #[test]
    fn raw_query_budgets_fail_closed() {
        Python::initialize();
        Python::attach(|py| {
            let database = database(fixture());
            assert!(database
                .query(
                    py,
                    "WITH RECURSIVE counter(value) AS (VALUES(1) UNION ALL SELECT value + 1 FROM counter) SELECT value FROM counter",
                    None,
                )
                .is_err());
            assert!(database
                .query(
                    py,
                    "SELECT printf('%0*d', 8388609, 0) AS oversized FROM decomposed_messages LIMIT 1",
                    None,
                )
                .is_err());
            let rows = database
                .query(py, "SELECT 1", None)
                .expect("normal query after budget failures");
            assert!(rows.bind(py).cast::<PyList>().expect("rows").len() < MAX_ROWS);
        });
    }

    #[test]
    fn default_path_is_host_scoped_not_workspace_scoped() {
        assert!(default_database_path().ends_with(".atm/db/mail.db"));
    }
}
