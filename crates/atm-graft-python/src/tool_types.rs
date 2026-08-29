//! Typed Python projections of canonical ATM daemon outcomes.

use atm_core::error::AtmErrorCode;
use atm_core::list::ListOutcome;
use atm_core::read::ReadOutcome;
use atm_core::send::WriteOutcome;
use pyo3::prelude::*;

use super::PyMessage;

/// Typed, JSON-compatible projection of the canonical send outcome.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct AtmSendResult {
    #[pyo3(get)]
    pub(crate) message_id: String,
    #[pyo3(get)]
    pub(crate) requires_ack: bool,
    #[pyo3(get)]
    pub(crate) outcome: String,
}

impl From<WriteOutcome> for AtmSendResult {
    fn from(outcome: WriteOutcome) -> Self {
        match outcome {
            WriteOutcome::Sent(outcome) => Self {
                message_id: outcome.message_id.to_string(),
                requires_ack: outcome.requires_ack,
                outcome: outcome.outcome.as_str().to_owned(),
            },
            WriteOutcome::Acknowledged(outcome) => Self {
                message_id: match outcome.reply_disposition {
                    atm_core::ack::AckReplyDisposition::Sent {
                        reply_message_id, ..
                    } => reply_message_id.to_string(),
                },
                requires_ack: false,
                outcome: "acknowledged".to_owned(),
            },
        }
    }
}

/// Typed, read-only projection of the canonical mailbox read outcome.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct AtmReadResult {
    #[pyo3(get)]
    pub(crate) count: usize,
    #[pyo3(get)]
    pub(crate) match_count: usize,
    #[pyo3(get)]
    pub(crate) additional_match_count: usize,
    #[pyo3(get)]
    pub(crate) mutation_applied: bool,
    #[pyo3(get)]
    pub(crate) message: Option<PyMessage>,
}

impl AtmReadResult {
    pub(crate) fn from_outcome(outcome: ReadOutcome) -> PyResult<Self> {
        let count = outcome.count;
        let match_count = outcome.match_count;
        let additional_match_count = outcome.additional_match_count;
        let mutation_applied = outcome.mutation_applied;
        let message = PyMessage::from_read(outcome)?.into_iter().next();
        Ok(Self {
            count,
            match_count,
            additional_match_count,
            mutation_applied,
            message,
        })
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
    #[pyo3(get)]
    pub(crate) from_agent: String,
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
            from_agent: row.from.to_string(),
            timestamp: row.timestamp.to_string(),
            read: row.read,
            pending_ack: row.pending_ack,
            task_id: row.task_id.map(|id| id.to_string()),
        }
    }
}

/// Typed, bounded projection of the canonical mailbox list outcome.
#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct AtmListResult {
    #[pyo3(get)]
    pub(crate) count: usize,
    #[pyo3(get)]
    pub(crate) rows: Vec<AtmListRow>,
}

impl From<ListOutcome> for AtmListResult {
    fn from(outcome: ListOutcome) -> Self {
        Self {
            count: outcome.count,
            rows: outcome.rows.into_iter().map(AtmListRow::from).collect(),
        }
    }
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
}

impl AtmToolError {
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

fn is_delivery_uncertain_code(code: &str) -> bool {
    matches!(
        code,
        value if value == AtmErrorCode::DaemonMayHaveExecuted.as_str()
            || value == AtmErrorCode::RemoteDeliveryUnconfirmed.as_str()
            || value == AtmErrorCode::WaitTimeout.as_str()
    )
}

#[cfg(test)]
mod tests {
    use atm_core::error::AtmErrorCode;
    use pyo3::exceptions::PyException;
    use pyo3::prelude::*;

    use super::AtmToolError;

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
        };
        assert!(daemon_unavailable.is_daemon_unavailable());

        let wait_timeout = AtmToolError {
            code: AtmErrorCode::WaitTimeout.as_str().to_owned(),
            message: "HTTP client request exceeded its absolute request budget".to_owned(),
            recovery: String::new(),
            layer: "native_client".to_owned(),
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
            };
            assert!(!error.is_daemon_unavailable());
        }
    }
}
