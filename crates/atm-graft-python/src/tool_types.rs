//! Typed Python projections of canonical ATM daemon outcomes.

use atm_core::error::AtmErrorCode;
use atm_core::list::ListOutcome;
use atm_core::read::ReadOutcome;
use atm_core::send::{SendOutcome, WriteOutcome};
use atm_core::types::{CommandAction, ReadSelection};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::OnceLock;

use super::PyMessage;
use crate::observability::ObservabilityStatus;

/// Diagnostic information attached when the fallback writer could not retain
/// an event. It is intentionally optional so successful calls remain compact.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct PyObservability {
    #[pyo3(get)]
    pub(crate) fallback_write_failed: bool,
    #[pyo3(get)]
    pub(crate) code: Option<String>,
}

/// Rust-resolved retained-log locations exposed to embedded hosts.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct PyObservabilityPaths {
    #[pyo3(get)]
    pub(crate) log_dir: String,
    #[pyo3(get)]
    pub(crate) canonical_log_path: String,
    #[pyo3(get)]
    pub(crate) fallback_log_path: String,
    #[pyo3(get)]
    pub(crate) log_dir_source: String,
}

/// Typed, JSON-compatible projection of the canonical send outcome.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct AtmSendResult {
    outcome_value: WriteOutcome,
    #[pyo3(get)]
    pub(crate) observability: Option<PyObservability>,
}

fn py_observability(status: ObservabilityStatus) -> Option<PyObservability> {
    status.fallback_write_failed.then_some(PyObservability {
        fallback_write_failed: true,
        code: status.code,
    })
}

impl AtmSendResult {
    pub(crate) fn with_observability(mut self, status: ObservabilityStatus) -> Self {
        self.observability = py_observability(status);
        self
    }

    fn send_outcome(&self) -> Option<&SendOutcome> {
        match &self.outcome_value {
            WriteOutcome::Sent(outcome) => Some(outcome),
            WriteOutcome::Acknowledged(_) => None,
        }
    }
}

impl From<WriteOutcome> for AtmSendResult {
    fn from(outcome: WriteOutcome) -> Self {
        Self {
            outcome_value: outcome,
            observability: None,
        }
    }
}

#[pymethods]
impl AtmSendResult {
    #[getter]
    fn action(&self) -> String {
        write_action(&self.outcome_value)
    }

    #[getter]
    fn team(&self) -> String {
        write_team(&self.outcome_value)
    }

    #[getter]
    fn agent(&self) -> String {
        write_agent(&self.outcome_value)
    }

    #[getter]
    fn sender(&self) -> Option<String> {
        self.send_outcome()
            .map(|outcome| outcome.sender.to_string())
    }

    #[getter]
    fn message_id(&self) -> String {
        write_message_id(&self.outcome_value)
    }

    #[getter]
    fn requires_ack(&self) -> bool {
        self.send_outcome()
            .is_some_and(|outcome| outcome.requires_ack)
    }

    #[getter]
    fn outcome(&self) -> String {
        match &self.outcome_value {
            WriteOutcome::Sent(outcome) => outcome.outcome.as_str().to_owned(),
            WriteOutcome::Acknowledged(_) => "acknowledged".to_owned(),
        }
    }

    #[getter]
    fn task_id(&self) -> Option<String> {
        write_task_id(&self.outcome_value)
    }

    #[getter]
    fn summary(&self) -> Option<String> {
        self.send_outcome()
            .and_then(|outcome| outcome.summary.clone())
    }

    #[getter]
    fn message(&self) -> Option<String> {
        self.send_outcome()
            .and_then(|outcome| outcome.message.clone())
    }

    #[getter]
    fn dry_run(&self) -> bool {
        self.send_outcome().is_some_and(|outcome| outcome.dry_run)
    }

    #[getter]
    fn reply_disposition(&self) -> Option<String> {
        match &self.outcome_value {
            WriteOutcome::Acknowledged(outcome) => Some(format!("{:?}", outcome.reply_disposition)),
            WriteOutcome::Sent(_) => None,
        }
    }

    #[getter]
    fn reply_text(&self) -> Option<String> {
        match &self.outcome_value {
            WriteOutcome::Acknowledged(outcome) => Some(outcome.reply_text.clone()),
            WriteOutcome::Sent(_) => None,
        }
    }

    fn to_json(&self) -> String {
        match &self.outcome_value {
            WriteOutcome::Sent(outcome) => serde_json::to_string(outcome),
            WriteOutcome::Acknowledged(outcome) => serde_json::to_string(outcome),
        }
        .expect("canonical ATM write outcome serializes")
    }
}

/// Typed, read-only projection of the canonical mailbox read outcome.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct AtmReadResult {
    outcome_value: ReadOutcome,
    #[pyo3(get)]
    pub(crate) message: Option<PyMessage>,
    #[pyo3(get)]
    pub(crate) observability: Option<PyObservability>,
}

impl AtmReadResult {
    pub(crate) fn with_observability(mut self, status: ObservabilityStatus) -> Self {
        self.observability = py_observability(status);
        self
    }

    pub(crate) fn from_outcome(outcome: ReadOutcome) -> PyResult<Self> {
        let message = PyMessage::from_read(outcome.clone())?.into_iter().next();
        Ok(Self {
            outcome_value: outcome,
            message,
            observability: None,
        })
    }

    /// Keep the public native-read envelope aligned with the CLI's `read`
    /// outcome while retaining the native tool's non-mutating peek request.
    pub(crate) fn from_peek_outcome(mut outcome: ReadOutcome) -> PyResult<Self> {
        outcome.action = CommandAction::Read;
        Self::from_outcome(outcome)
    }
}

#[pymethods]
impl AtmReadResult {
    #[getter]
    fn action(&self) -> String {
        action(self.outcome_value.action).to_owned()
    }

    #[getter]
    fn team(&self) -> String {
        self.outcome_value.team.to_string()
    }

    #[getter]
    fn agent(&self) -> String {
        self.outcome_value.agent.to_string()
    }

    #[getter]
    fn selection_mode(&self) -> String {
        read_selection(self.outcome_value.selection_mode).to_owned()
    }

    #[getter]
    fn mutation_applied(&self) -> bool {
        self.outcome_value.mutation_applied
    }

    #[getter]
    fn count(&self) -> usize {
        self.outcome_value.count
    }

    #[getter]
    fn selected_message_id(&self) -> Option<String> {
        self.outcome_value
            .selected_message_id
            .map(|id| id.to_string())
    }

    #[getter]
    fn match_count(&self) -> usize {
        self.outcome_value.match_count
    }

    #[getter]
    fn additional_match_count(&self) -> usize {
        self.outcome_value.additional_match_count
    }

    #[getter]
    fn bucket_counts<'py>(&self, py: Python<'py>) -> Bound<'py, PyDict> {
        bucket_counts_dict(py, &self.outcome_value.bucket_counts)
    }

    fn to_json(&self) -> String {
        serde_json::to_string(&self.outcome_value).expect("canonical ATM read outcome serializes")
    }
}

/// One typed row in a bounded native mailbox list result.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct AtmListRow {
    #[pyo3(get)]
    pub(crate) message_id: Option<String>,
    #[pyo3(get)]
    pub(crate) summary: String,
    from: String,
    #[pyo3(get)]
    pub(crate) timestamp: String,
    #[pyo3(get)]
    pub(crate) read: bool,
    #[pyo3(get)]
    pub(crate) pending_ack: bool,
    #[pyo3(get)]
    pub(crate) task_id: Option<String>,
}

impl From<atm_core::list::ListRow> for AtmListRow {
    fn from(row: atm_core::list::ListRow) -> Self {
        Self {
            message_id: row.message_id.map(|id| id.to_string()),
            summary: row.summary,
            from: row.from.to_string(),
            timestamp: canonical_timestamp(&row.timestamp),
            read: row.read,
            pending_ack: row.pending_ack,
            task_id: row.task_id.map(|id| id.to_string()),
        }
    }
}

static FROM_AGENT_WARNING: OnceLock<()> = OnceLock::new();

#[pymethods]
impl AtmListRow {
    #[getter(from)]
    fn agent_name(&self) -> String {
        self.from.clone()
    }

    #[getter(from_agent)]
    fn agent_name_deprecated(&self, py: Python<'_>) -> PyResult<String> {
        if FROM_AGENT_WARNING.set(()).is_ok() {
            let warnings = py.import("warnings")?;
            let warning_type = py.get_type::<pyo3::exceptions::PyDeprecationWarning>();
            warnings.call_method1(
                "warn",
                (
                    "AtmListRow.from_agent is deprecated; use row.from",
                    warning_type,
                ),
            )?;
        }
        Ok(self.from.clone())
    }
}

/// Typed, bounded projection of the canonical mailbox list outcome.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct AtmListResult {
    outcome_value: ListOutcome,
    #[pyo3(get)]
    pub(crate) rows: Vec<AtmListRow>,
    #[pyo3(get)]
    pub(crate) observability: Option<PyObservability>,
}

impl From<ListOutcome> for AtmListResult {
    fn from(outcome: ListOutcome) -> Self {
        Self {
            rows: outcome.rows.iter().cloned().map(AtmListRow::from).collect(),
            outcome_value: outcome,
            observability: None,
        }
    }
}

#[pymethods]
impl AtmListResult {
    #[getter]
    fn action(&self) -> String {
        action(self.outcome_value.action).to_owned()
    }

    #[getter]
    fn team(&self) -> String {
        self.outcome_value.team.to_string()
    }

    #[getter]
    fn agent(&self) -> String {
        self.outcome_value.agent.to_string()
    }

    #[getter]
    fn selection_mode(&self) -> String {
        read_selection(self.outcome_value.selection_mode).to_owned()
    }

    #[getter]
    fn history_collapsed(&self) -> bool {
        self.outcome_value.history_collapsed
    }

    #[getter]
    fn count(&self) -> usize {
        self.outcome_value.count
    }

    #[getter]
    fn bucket_counts<'py>(&self, py: Python<'py>) -> Bound<'py, PyDict> {
        bucket_counts_dict(py, &self.outcome_value.bucket_counts)
    }

    fn to_json(&self) -> String {
        serde_json::to_string(&self.outcome_value).expect("canonical ATM list outcome serializes")
    }
}

impl AtmListResult {
    pub(crate) fn with_observability(mut self, status: ObservabilityStatus) -> Self {
        self.observability = py_observability(status);
        self
    }

    pub(crate) fn count_value(&self) -> usize {
        self.outcome_value.count
    }
}

pub(crate) fn canonical_timestamp(value: &atm_core::types::IsoTimestamp) -> String {
    let value = value.to_string();
    value
        .strip_suffix("+00:00")
        .map_or(value.clone(), |prefix| format!("{prefix}Z"))
}

fn read_selection(value: ReadSelection) -> &'static str {
    match value {
        ReadSelection::Actionable => "actionable",
        ReadSelection::Unread => "unread",
        ReadSelection::PendingAck => "pending_ack",
        ReadSelection::All => "all",
    }
}

fn action(value: CommandAction) -> &'static str {
    match value {
        CommandAction::Ack => "ack",
        CommandAction::Clear => "clear",
        CommandAction::List => "list",
        CommandAction::Peek => "peek",
        CommandAction::Read => "read",
        CommandAction::Send => "send",
    }
}

fn write_action(value: &WriteOutcome) -> String {
    match value {
        WriteOutcome::Sent(outcome) => action(outcome.action),
        WriteOutcome::Acknowledged(outcome) => action(outcome.action),
    }
    .to_owned()
}

fn write_team(value: &WriteOutcome) -> String {
    match value {
        WriteOutcome::Sent(outcome) => outcome.team.to_string(),
        WriteOutcome::Acknowledged(outcome) => outcome.team.to_string(),
    }
}

fn write_agent(value: &WriteOutcome) -> String {
    match value {
        WriteOutcome::Sent(outcome) => outcome.agent.to_string(),
        WriteOutcome::Acknowledged(outcome) => outcome.agent.to_string(),
    }
}

fn write_message_id(value: &WriteOutcome) -> String {
    match value {
        WriteOutcome::Sent(outcome) => outcome.message_id.to_string(),
        WriteOutcome::Acknowledged(outcome) => outcome.message_id.to_string(),
    }
}

fn write_task_id(value: &WriteOutcome) -> Option<String> {
    match value {
        WriteOutcome::Sent(outcome) => outcome.task_id.as_ref().map(ToString::to_string),
        WriteOutcome::Acknowledged(outcome) => outcome.task_id.as_ref().map(ToString::to_string),
    }
}

fn bucket_counts_dict<'py>(
    py: Python<'py>,
    counts: &atm_core::read::BucketCounts,
) -> Bound<'py, PyDict> {
    let result = PyDict::new(py);
    result
        .set_item("unread", counts.unread)
        .expect("dict accepts integer");
    result
        .set_item("pending_ack", counts.pending_ack)
        .expect("dict accepts integer");
    result
        .set_item("history", counts.history)
        .expect("dict accepts integer");
    result
}

/// Structured native-tool error data used by Python adapters' failure envelope.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct AtmToolError {
    #[pyo3(get)]
    pub(crate) code: String,
    #[pyo3(get)]
    pub(crate) message: String,
    #[pyo3(get)]
    pub(crate) recovery: String,
    #[pyo3(get)]
    pub(crate) layer: String,
    #[pyo3(get)]
    pub(crate) observability: Option<PyObservability>,
}

impl AtmToolError {
    pub(crate) fn with_observability(mut self, status: ObservabilityStatus) -> Self {
        self.observability = py_observability(status);
        self
    }

    pub(crate) fn from_native_error(py: Python<'_>, error: &PyErr) -> Self {
        let value = error.value(py);
        let attribute = |name: &str| {
            value
                .getattr(name)
                .ok()
                .and_then(|attribute| attribute.extract::<String>().ok())
        };
        let code =
            attribute("code").unwrap_or_else(|| AtmErrorCode::InternalError.as_str().to_owned());
        let recovery = if is_delivery_uncertain_code(&code) {
            "the request outcome is uncertain; inspect mailbox or service-side effects before attempting it again"
        } else {
            "verify the local ATM daemon and configured identity, then retry"
        };
        Self {
            code,
            message: attribute("message").unwrap_or_else(|| error.to_string()),
            recovery: recovery.to_owned(),
            layer: "native_client".to_owned(),
            observability: None,
        }
    }

    pub(crate) fn is_daemon_unavailable(&self) -> bool {
        self.code == AtmErrorCode::DaemonUnavailable.as_str()
    }

    pub(crate) fn with_recovery(mut self, recovery: impl Into<String>) -> Self {
        self.recovery = recovery.into();
        self
    }
}

const DELIVERY_UNCERTAIN_CODES: [AtmErrorCode; 3] = [
    AtmErrorCode::DaemonMayHaveExecuted,
    AtmErrorCode::RemoteDeliveryUnconfirmed,
    AtmErrorCode::WaitTimeout,
];

fn is_delivery_uncertain_code(code: &str) -> bool {
    match code.parse::<AtmErrorCode>() {
        Ok(code) => DELIVERY_UNCERTAIN_CODES.contains(&code),
        // An unknown future code must not receive recovery text that invites
        // a potentially duplicating retry. Treat it as delivery-uncertain
        // until the canonical registry classifies it explicitly.
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use atm_core::error::AtmErrorCode;
    use atm_core::types::IsoTimestamp;
    use pyo3::exceptions::PyException;
    use pyo3::prelude::*;
    use pyo3::types::PyDict;

    use super::{AtmListRow, AtmToolError, canonical_timestamp, is_delivery_uncertain_code};

    #[test]
    fn canonical_timestamps_use_cli_z_suffix() {
        let timestamp = canonical_timestamp(&IsoTimestamp::now());

        assert!(timestamp.ends_with('Z'));
        assert!(!timestamp.ends_with("+00:00"));
    }

    #[test]
    fn deprecated_from_agent_warns_once_and_matches_from() {
        Python::initialize();
        Python::attach(|py| {
            let row = AtmListRow {
                message_id: None,
                summary: "summary".to_owned(),
                from: "source-agent".to_owned(),
                timestamp: "2026-09-05T00:00:00Z".to_owned(),
                read: false,
                pending_ack: true,
                task_id: None,
            };
            let warnings = py.import("warnings").expect("warnings module");
            let options = PyDict::new(py);
            options
                .set_item("record", true)
                .expect("warning capture options");
            let context = warnings
                .call_method("catch_warnings", (), Some(&options))
                .expect("warning capture context");
            let records = context
                .call_method0("__enter__")
                .expect("enter warning capture context");
            warnings
                .call_method1("simplefilter", ("always",))
                .expect("enable warning capture");

            assert_eq!(row.agent_name(), "source-agent");
            assert_eq!(
                row.agent_name_deprecated(py).expect("deprecated getter"),
                row.agent_name()
            );
            assert_eq!(
                row.agent_name_deprecated(py).expect("deprecated getter"),
                row.agent_name()
            );

            context
                .call_method1("__exit__", (py.None(), py.None(), py.None()))
                .expect("exit warning capture context");
            assert_eq!(records.len().expect("warning record length"), 1);
        });
    }

    #[test]
    fn unstructured_python_errors_use_the_canonical_internal_error_code() {
        Python::initialize();
        Python::attach(|py| {
            let error = PyErr::new::<PyException, _>("unstructured extension failure");
            let result = AtmToolError::from_native_error(py, &error);

            assert_eq!(result.code, AtmErrorCode::InternalError.as_str());
            assert_eq!(result.layer, "native_client");
        });
    }

    #[test]
    fn unknown_error_codes_are_treated_as_delivery_uncertain() {
        assert!(is_delivery_uncertain_code(
            AtmErrorCode::DaemonMayHaveExecuted.as_str()
        ));
        assert!(is_delivery_uncertain_code(
            AtmErrorCode::RemoteDeliveryUnconfirmed.as_str()
        ));
        assert!(is_delivery_uncertain_code(
            AtmErrorCode::WaitTimeout.as_str()
        ));
        assert!(is_delivery_uncertain_code("ATM_FUTURE_UNSPECIFIED"));
        assert!(!is_delivery_uncertain_code(
            AtmErrorCode::DaemonUnavailable.as_str()
        ));
    }

    /// Only the pre-send local-connect code enters stale-client recovery.
    /// An uncertain request-write result must stay outside that path because
    /// the daemon may already have accepted the request.
    #[test]
    fn only_the_daemon_unavailable_code_is_treated_as_a_recoverable_stale_client() {
        let daemon_unavailable = AtmToolError {
            code: AtmErrorCode::DaemonUnavailable.as_str().to_owned(),
            message: "HTTP client could not connect to the configured daemon endpoint".to_owned(),
            recovery: String::new(),
            layer: "native_client".to_owned(),
            observability: None,
        };
        assert!(daemon_unavailable.is_daemon_unavailable());

        let wait_timeout = AtmToolError {
            code: AtmErrorCode::WaitTimeout.as_str().to_owned(),
            message: "HTTP client request exceeded its absolute request budget".to_owned(),
            recovery: String::new(),
            layer: "native_client".to_owned(),
            observability: None,
        };
        assert!(
            !wait_timeout.is_daemon_unavailable(),
            "a request-budget timeout may mean the write already reached the server; \
             it must never be classified as safe to silently retry"
        );

        let uncertain_write = AtmToolError {
            code: AtmErrorCode::DaemonMayHaveExecuted.as_str().to_owned(),
            message: "request acceptance is unknown".to_owned(),
            recovery: "inspect mailbox or service-side effects before attempting it again"
                .to_owned(),
            layer: "native_client".to_owned(),
            observability: None,
        };
        assert!(!uncertain_write.is_daemon_unavailable());

        for code in [
            AtmErrorCode::RemoteDeliveryUnconfirmed,
            AtmErrorCode::WaitTimeout,
        ] {
            let error = AtmToolError {
                code: code.as_str().to_owned(),
                message: "request outcome is uncertain".to_owned(),
                recovery: String::new(),
                layer: "native_client".to_owned(),
                observability: None,
            };
            assert!(!error.is_daemon_unavailable());
        }
    }
}
