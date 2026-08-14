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
    use atm_storage_rusqlite::{
        create_an8_analyst_query_fixture_for_test, create_analyst_query_fixture_for_test,
        open_analyst_query_store,
    };
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyList, PyTuple};
    use tempfile::tempdir;

    const SELECTIVE_TEAM: &str = "fixture-team";
    const UNMATCHED_TEAM: &str = "fixture-no-match";

    fn fixture() -> std::path::PathBuf {
        let directory = tempdir().expect("temp directory").keep();
        let path = directory.join("mail.db");
        create_analyst_query_fixture_for_test(&path).expect("fixture database");
        path
    }

    fn an8_fixture() -> std::path::PathBuf {
        let directory = tempdir().expect("temp directory").keep();
        let path = directory.join("mail.db");
        create_an8_analyst_query_fixture_for_test(&path).expect("AN.8 fixture database");
        path
    }

    fn database(path: std::path::PathBuf) -> ReadonlyDatabase {
        ReadonlyDatabase {
            store: open_analyst_query_store(path).expect("read-only store"),
        }
    }

    fn query_artifact<'py>(
        database: &ReadonlyDatabase,
        py: Python<'py>,
        sql: &str,
    ) -> Bound<'py, PyList> {
        database
            .query(py, sql, None)
            .expect("committed AN.8 query artifact executes through the Python surface")
            .bind(py)
            .clone()
            .cast_into::<PyList>()
            .expect("query rows")
    }

    fn row<'py>(rows: &Bound<'py, PyList>, index: usize) -> Bound<'py, PyDict> {
        rows.get_item(index)
            .expect("row")
            .cast_into::<PyDict>()
            .expect("row mapping")
    }

    fn text(row: &Bound<'_, PyDict>, key: &str) -> String {
        row.get_item(key)
            .expect("mapping lookup")
            .expect("column exists")
            .extract::<String>()
            .expect("text column")
    }

    fn integer(row: &Bound<'_, PyDict>, key: &str) -> i64 {
        row.get_item(key)
            .expect("mapping lookup")
            .expect("column exists")
            .extract::<i64>()
            .expect("integer column")
    }

    fn expected_rows<'py>(expected: &Bound<'py, PyDict>, key: &str) -> Bound<'py, PyList> {
        expected
            .get_item(key)
            .expect("expected result key")
            .expect("expected result value")
            .cast_into::<PyList>()
            .expect("expected-result array")
    }

    fn expected_results<'py>(py: Python<'py>) -> Bound<'py, PyDict> {
        py.import("json")
            .expect("Python JSON module")
            .call_method1(
                "loads",
                (include_str!(
                    "../../../docs/plans/phase-an/fixtures/queries/expected-results.json"
                ),),
            )
            .expect("parse hand-calculated AN.8 expected results")
            .cast_into::<PyDict>()
            .expect("expected-result mapping")
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
            let parameters = PyTuple::new(py, [SELECTIVE_TEAM]).expect("parameters");
            let result = database
                .query(
                    py,
                    "SELECT team, value FROM decomposed_messages WHERE team = ?",
                    Some(parameters),
                )
                .expect("query");
            let list = result.bind(py).clone().cast_into::<PyList>().expect("list");
            assert_eq!(list.len(), 1);
            assert_eq!(
                list.get_item(0)
                    .expect("row")
                    .get_item("value")
                    .expect("value")
                    .extract::<String>()
                    .expect("text"),
                "fixture assignment"
            );

            let wrong_parameters = PyTuple::new(py, [UNMATCHED_TEAM]).expect("parameters");
            let wrong_result = database
                .query(
                    py,
                    "SELECT team, value FROM decomposed_messages WHERE team = ?",
                    Some(wrong_parameters),
                )
                .expect("query with a non-matching bound team");
            assert!(
                wrong_result
                    .bind(py)
                    .cast::<PyList>()
                    .expect("list")
                    .is_empty()
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
    fn parameterized_queries_can_read_each_explicit_tag_provenance_column() {
        Python::initialize();
        Python::attach(|py| {
            let database = database(fixture());
            let parameters = PyTuple::new(py, [SELECTIVE_TEAM]).expect("parameters");
            let rows = database
                .query(
                    py,
                    "SELECT instance_tags_json, applied_template_tags_json, derived_tags_json, effective_tags_json \
                     FROM decomposed_messages WHERE team = ? ORDER BY agent LIMIT 1",
                    Some(parameters),
                )
                .expect("provenance query");
            let row = rows
                .bind(py)
                .cast::<PyList>()
                .expect("rows")
                .get_item(0)
                .expect("row")
                .cast_into::<PyDict>()
                .expect("mapping");
            for field in [
                "instance_tags_json",
                "applied_template_tags_json",
                "derived_tags_json",
                "effective_tags_json",
            ] {
                assert!(
                    row.get_item(field).expect("column access").is_some(),
                    "{field}"
                );
            }
        });
    }

    #[test]
    fn an8_motivating_query_artifacts_return_hand_calculated_results() {
        // The source is embedded at compile time: the analyst query itself only
        // opens SQLite through the read-only `ReadonlyDatabase` boundary.
        Python::initialize();
        Python::attach(|py| {
            let database = database(an8_fixture());
            let expected = expected_results(py);

            let spans = query_artifact(
                &database,
                py,
                include_str!("../../../docs/plans/phase-an/fixtures/queries/q1-sprint-span.sql"),
            );
            let expected_spans = expected_rows(&expected, "q1_sprint_span");
            assert_eq!(spans.len(), expected_spans.len());
            for (index, expected_row) in expected_spans.iter().enumerate() {
                let expected_row = expected_row
                    .cast_into::<PyDict>()
                    .expect("expected row mapping");
                let actual = row(&spans, index);
                for key in ["sprint", "first_assignment_at", "completion_at"] {
                    assert_eq!(
                        text(&actual, key),
                        text(&expected_row, key),
                        "Q1 {key} at row {index}"
                    );
                }
            }

            let iterations = query_artifact(
                &database,
                py,
                include_str!("../../../docs/plans/phase-an/fixtures/queries/q2-qa-iterations.sql"),
            );
            let expected_iterations = expected_rows(&expected, "q2_qa_iterations");
            assert_eq!(iterations.len(), expected_iterations.len());
            for (index, expected_row) in expected_iterations.iter().enumerate() {
                let expected_row = expected_row
                    .cast_into::<PyDict>()
                    .expect("expected row mapping");
                let actual = row(&iterations, index);
                assert_eq!(text(&actual, "sprint"), text(&expected_row, "sprint"));
                assert_eq!(
                    integer(&actual, "qa_iterations"),
                    integer(&expected_row, "qa_iterations")
                );
            }

            let findings = query_artifact(
                &database,
                py,
                include_str!(
                    "../../../docs/plans/phase-an/fixtures/queries/q3-findings-by-severity.sql"
                ),
            );
            let expected_findings = expected_rows(&expected, "q3_findings_by_severity");
            assert_eq!(findings.len(), expected_findings.len());
            for (index, expected_row) in expected_findings.iter().enumerate() {
                let expected_row = expected_row
                    .cast_into::<PyDict>()
                    .expect("expected row mapping");
                let actual = row(&findings, index);
                for key in ["sprint", "severity"] {
                    assert_eq!(
                        text(&actual, key),
                        text(&expected_row, key),
                        "Q3 {key} at row {index}"
                    );
                }
                for key in ["qa_round", "findings"] {
                    assert_eq!(
                        integer(&actual, key),
                        integer(&expected_row, key),
                        "Q3 {key} at row {index}"
                    );
                }
            }

            let developers = query_artifact(
                &database,
                py,
                include_str!(
                    "../../../docs/plans/phase-an/fixtures/queries/q4-developer-by-sprint.sql"
                ),
            );
            let expected_developers = expected_rows(&expected, "q4_developer_by_sprint");
            assert_eq!(developers.len(), expected_developers.len());
            for (index, expected_row) in expected_developers.iter().enumerate() {
                let expected_row = expected_row
                    .cast_into::<PyDict>()
                    .expect("expected row mapping");
                let actual = row(&developers, index);
                for key in ["sprint", "developer"] {
                    assert_eq!(
                        text(&actual, key),
                        text(&expected_row, key),
                        "Q4 {key} at row {index}"
                    );
                }
            }
        });
    }

    #[test]
    fn an8_synthetic_vocabulary_uses_the_same_generic_query_surface() {
        Python::initialize();
        Python::attach(|py| {
            let database = database(an8_fixture());
            let rows = query_artifact(
                &database,
                py,
                include_str!(
                    "../../../docs/plans/phase-an/fixtures/queries/synthetic-vocabulary.sql"
                ),
            );
            assert_eq!(rows.len(), 1);
            let result = row(&rows, 0);
            assert_eq!(text(&result, "cycle"), "PX-7");
            assert_eq!(text(&result, "opened_at"), "2026-08-03T08:00:00Z");
            assert_eq!(text(&result, "delivered_at"), "2026-08-03T10:00:00Z");
            assert_eq!(text(&result, "owner"), "owner-rose");
            assert_eq!(integer(&result, "high_risk_assessments"), 1);
        });
    }

    #[test]
    fn an8_query_corpus_keeps_the_captured_template_contract() {
        let task_template =
            include_str!("../../../docs/plans/phase-an/fixtures/task-assignment.xml.j2");
        let qa_template = include_str!("../../../docs/plans/phase-an/fixtures/qa-report.xml.j2");
        assert!(task_template.contains("name: dev-task"));
        assert!(qa_template.contains("name: qa-task"));

        Python::initialize();
        Python::attach(|py| {
            let database = database(an8_fixture());
            let rows = query_artifact(
                &database,
                py,
                "SELECT DISTINCT template_type FROM decomposed_messages \
                 WHERE team = 'fixture-an8' ORDER BY template_type",
            );
            assert_eq!(rows.len(), 2);
            assert_eq!(text(&row(&rows, 0), "template_type"), "dev-task");
            assert_eq!(text(&row(&rows, 1), "template_type"), "qa-task");
        });
    }

    #[test]
    fn an8_replaces_the_captured_file_oriented_helper_with_sqlite_queries() {
        // AN.1's capture is a JSON mailbox reader/atomic writer, not a query
        // parser. Keep that historical fact explicit so validation never
        // fabricates parser-answer equivalence that the source cannot have.
        let historical_helper =
            include_str!("../../../docs/plans/phase-an/fixtures/claude_inbox_tmpfile_parser.py");
        assert!(historical_helper.contains("json.loads"));
        assert!(historical_helper.contains("NamedTemporaryFile"));
        assert!(!historical_helper.contains("decomposed_messages"));

        let query_surface = include_str!("lib.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production query surface");
        assert!(query_surface.contains("open_analyst_query_store"));
        assert!(!query_surface.contains("std::fs"));
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
