//! Python translation layer over the typed `atm-graft` client.
//!
//! This crate does not open a daemon socket or access storage itself. Every
//! operation delegates to the existing graft client and its canonical API
//! request types.

use std::sync::{Arc, Mutex, OnceLock};

use ::atm_graft::{
    GraftClient, GraftSession, GraftSessionOptions, GraftSessionState, HostNudge,
    HostNudgeInjector, MailboxWorkCounts, SessionSnapshot,
};
use atm_core::address::AgentAddress;
use atm_core::boundary::{NudgeKind, PostSendHookEvent};
use atm_core::caller_context::activity_observation_for_resolved_caller;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::AtmGraftClient;
use atm_core::home::{atm_home, command_invocation_dir};
use atm_core::list::ListQuery;
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::schema::AtmMessageId;
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::types::{AgentName, ChatId, IsoTimestamp, ReadSelection, TeamName};
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

mod message;
mod observability;
mod query;
mod recovery;
mod tool_types;

use message::PyMessage;
pub use tool_types::AtmSendResult as AtmAckResult;
pub use tool_types::{
    AtmListResult, AtmListRow, AtmReadResult, AtmSendResult, AtmToolError, PyObservability,
    PyObservabilityPaths,
};

fn python_extension_runtime() -> PyResult<&'static Mutex<tokio::runtime::Runtime>> {
    // Python calls may arrive from arbitrary host threads. Serialize the one
    // current-thread runtime instead of creating a competing runtime per call.
    static RUNTIME: OnceLock<Mutex<tokio::runtime::Runtime>> = OnceLock::new();
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map(Mutex::new)
        .map_err(|error| {
            atm_error(
                AtmError::new(
                    AtmErrorCode::InternalError,
                    "failed to create the ATM Python extension runtime",
                )
                .with_cause(error),
            )
        })?;
    Ok(RUNTIME.get_or_init(|| runtime))
}

// Python projection of ATM's canonical structured error contract. The
// exception message remains useful in ordinary Python tracebacks, while
// callers that need machine-readable handling use `code`, `message`, and
// optional `cause` directly.
pyo3::create_exception!(atm_graft, AtmGraftError, PyException);

fn atm_error(error: AtmError) -> PyErr {
    Python::attach(|py| {
        let code = error.code().as_str();
        let message = error.message().to_owned();
        let cause = error.cause().map(str::to_owned);
        let py_error = AtmGraftError::new_err(message.clone());
        let value = py_error.value(py);

        // Every ATM exception type has an instance dictionary. Failure to set
        // these fields would be a binding implementation defect, not a caller
        // error, so retain the canonical traceback rather than masking it.
        value
            .setattr("code", code)
            .expect("ATM Python exception accepts a code field");
        value
            .setattr("message", message)
            .expect("ATM Python exception accepts a message field");
        value
            .setattr("cause", cause)
            .expect("ATM Python exception accepts a cause field");
        py_error
    })
}

fn poisoned_lock_error(lock_name: &'static str) -> PyErr {
    atm_error(AtmError::new(
        AtmErrorCode::InternalError,
        format!("{lock_name} lock poisoned"),
    ))
}

fn python_callback_error(error: PyErr) -> AtmError {
    AtmError::new(
        AtmErrorCode::InternalError,
        "Python graft nudge callback failed",
    )
    .with_cause(error.to_string())
}

#[pyclass(from_py_object)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PyAgentAddress {
    #[pyo3(get)]
    agent: String,
    #[pyo3(get)]
    chat_id: Option<String>,
    #[pyo3(get)]
    team: String,
}

impl PyAgentAddress {
    fn to_typed(&self) -> PyResult<AgentAddress> {
        AgentAddress::new(
            self.agent.parse::<AgentName>().map_err(atm_error)?,
            self.chat_id
                .as_deref()
                .map(str::parse::<ChatId>)
                .transpose()
                .map_err(atm_error)?,
            Some(self.team.parse::<TeamName>().map_err(atm_error)?),
            None,
        )
        .map_err(atm_error)
    }

    fn from_typed(address: AgentAddress) -> PyResult<Self> {
        let team = address
            .team()
            .cloned()
            .ok_or_else(|| atm_error(AtmError::validation("ATM address requires a team")))?;
        Ok(Self {
            agent: address.agent().to_string(),
            chat_id: address.chat_id().map(ToString::to_string),
            team: team.to_string(),
        })
    }
}

#[pyclass(from_py_object)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PyGraftSessionOptions {
    #[pyo3(get)]
    workspace_root: String,
    #[pyo3(get)]
    agent: String,
    #[pyo3(get)]
    team: String,
    typed_team: TeamName,
    typed_agent: AgentName,
}

impl PyGraftSessionOptions {
    fn to_typed(&self) -> GraftSessionOptions {
        GraftSessionOptions::new(
            &self.workspace_root,
            self.typed_team.clone(),
            self.typed_agent.clone(),
        )
    }
}

#[pymethods]
impl PyGraftSessionOptions {
    #[new]
    fn new(workspace_root: String, agent: String, team: String) -> PyResult<Self> {
        if workspace_root.trim().is_empty() {
            return Err(atm_error(AtmError::validation(
                "workspace_root must not be blank",
            )));
        }
        let typed_team = team.parse::<TeamName>().map_err(atm_error)?;
        let typed_agent = agent.parse::<AgentName>().map_err(atm_error)?;
        Ok(Self {
            workspace_root,
            agent,
            team,
            typed_team,
            typed_agent,
        })
    }
}

#[pymethods]
impl PyAgentAddress {
    #[new]
    fn new(agent: String, team: String, chat_id: Option<String>) -> PyResult<Self> {
        let address = Self {
            agent,
            chat_id,
            team,
        };
        address.to_typed()?;
        Ok(address)
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(self.to_typed()?.to_string())
    }
}

#[derive(Clone, Copy)]
enum DaemonRecoveryPolicy {
    /// Refresh the client but do not replay a possibly accepted write.
    RefreshOnly,
    /// Refresh the client and replay one read-only request.
    RetryOnce,
}

enum DaemonRecovery<T> {
    Completed(T, observability::ObservabilityStatus),
    Failed {
        error: PyErr,
        refreshed: bool,
        refresh_error: Option<PyErr>,
        observability: observability::ObservabilityStatus,
    },
}

#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct PyNudge {
    #[pyo3(get)]
    message_id: String,
    #[pyo3(get)]
    source: PyAgentAddress,
    #[pyo3(get)]
    body: String,
    #[pyo3(get)]
    notice_text: String,
    /// ATM's additive nudge taxonomy projection: `steer` or `queue`.
    #[pyo3(get)]
    kind: String,
}

impl PyNudge {
    pub fn from_post_send(event: &PostSendHookEvent) -> PyResult<Self> {
        let body = event.description.clone();
        Ok(Self {
            message_id: event.message_id.to_string(),
            source: PyAgentAddress::from_typed(event.source_address())?,
            notice_text: body.clone(),
            body,
            kind: NudgeKind::Steer.as_str().to_owned(),
        })
    }

    pub fn from_host_nudge(nudge: &HostNudge) -> PyResult<Self> {
        Ok(Self {
            message_id: nudge.event.message_id.to_string(),
            source: PyAgentAddress::from_typed(nudge.event.source_address())?,
            body: nudge.body.clone(),
            notice_text: nudge.notice_text.clone(),
            kind: nudge.kind.as_str().to_owned(),
        })
    }
}

#[pymethods]
impl PyNudge {
    #[new]
    #[pyo3(signature = (message_id, source, body, kind="steer"))]
    fn new(message_id: String, source: PyAgentAddress, body: String, kind: &str) -> PyResult<Self> {
        message_id
            .parse::<atm_core::schema::AtmMessageId>()
            .map_err(|error| {
                atm_error(AtmError::validation(format!("invalid message id: {error}")))
            })?;
        if body.trim().is_empty() {
            return Err(atm_error(AtmError::validation(
                "nudge body must not be blank",
            )));
        }
        let kind = match kind {
            "steer" | "queue" => kind.to_owned(),
            _ => {
                return Err(atm_error(AtmError::validation(
                    "nudge kind must be `steer` or `queue`",
                )));
            }
        };
        Ok(Self {
            message_id,
            source,
            notice_text: body.clone(),
            body,
            kind,
        })
    }

    fn __repr__(&self) -> String {
        format!("PyNudge(message_id={})", self.message_id)
    }
}

#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct PyGraftSessionSnapshot {
    #[pyo3(get)]
    agent: String,
    #[pyo3(get)]
    team: String,
    #[pyo3(get)]
    state: String,
}

/// Python count-only projection of durable mailbox work for recovery notices.
#[pyclass(from_py_object)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PyMailboxWorkCounts {
    #[pyo3(get)]
    unread: usize,
    #[pyo3(get)]
    pending_ack: usize,
}

impl From<MailboxWorkCounts> for PyMailboxWorkCounts {
    fn from(counts: MailboxWorkCounts) -> Self {
        Self {
            unread: counts.unread,
            pending_ack: counts.pending_ack,
        }
    }
}

impl From<SessionSnapshot> for PyGraftSessionSnapshot {
    fn from(snapshot: SessionSnapshot) -> Self {
        let state = match snapshot.state {
            GraftSessionState::Inactive => "inactive",
            GraftSessionState::Listening => "listening",
            GraftSessionState::Degraded => "degraded",
            GraftSessionState::Closed => "closed",
            GraftSessionState::CloseFailed => "close_failed",
        };
        Self {
            agent: snapshot.agent.to_string(),
            team: snapshot.team.to_string(),
            state: state.to_string(),
        }
    }
}

struct PythonNudgeInjector {
    callback: Py<PyAny>,
}

impl HostNudgeInjector for PythonNudgeInjector {
    fn inject_nudge(&self, nudge: &HostNudge) -> Result<(), AtmError> {
        Python::attach(|py| {
            let nudge = Py::new(
                py,
                PyNudge::from_host_nudge(nudge).map_err(python_callback_error)?,
            )
            .map_err(python_callback_error)?;
            self.callback
                .call1(py, (nudge,))
                .map(|_| ())
                .map_err(python_callback_error)
        })
    }
}

#[pyclass(skip_from_py_object)]
pub struct PyGraftSession {
    caller: AgentAddress,
    // PyO3 methods can cross the GIL from multiple Python threads; these
    // mutable lifecycle handles therefore need real synchronization.
    client: Mutex<Option<GraftClient>>,
    receiver: Mutex<Option<GraftSession>>,
    fallback_logger: Arc<observability::GraftFallbackLogger>,
    #[cfg(test)]
    reconnect_replacement: Mutex<Option<GraftClient>>,
    #[cfg(test)]
    reconnect_attempts: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    reconnect_fallback_attempts: std::sync::atomic::AtomicUsize,
}

fn observability_log_dir() -> PyResult<(std::path::PathBuf, &'static str)> {
    if std::env::var_os("ATM_LOG_DIR").is_some_and(|value| !value.is_empty()) {
        return atm_core::home::host_log_dir()
            .map(|path| (path, "env:ATM_LOG_DIR"))
            .map_err(atm_error);
    }
    if std::env::var_os("ATM_HOME").is_some_and(|value| !value.is_empty()) {
        let home = atm_home().map_err(atm_error)?;
        return Ok((
            atm_core::home::host_log_dir_from_home(&home),
            "env:ATM_HOME",
        ));
    }
    atm_core::home::host_log_dir()
        .map(|path| (path, "default"))
        .map_err(atm_error)
}

#[pyfunction]
fn observability_paths() -> PyResult<PyObservabilityPaths> {
    let (log_dir, source) = observability_log_dir()?;
    Ok(PyObservabilityPaths {
        canonical_log_path: log_dir
            .join(atm_observability::CANONICAL_LOG_FILE_NAME)
            .display()
            .to_string(),
        fallback_log_path: observability::fallback_path(&log_dir).display().to_string(),
        log_dir: log_dir.display().to_string(),
        log_dir_source: source.to_owned(),
    })
}

impl PyGraftSession {
    fn client(&self) -> PyResult<GraftClient> {
        self.client
            .lock()
            .map_err(|_| poisoned_lock_error("ATM graft session"))?
            .clone()
            .ok_or_else(|| atm_error(AtmError::daemon_unavailable("ATM graft session is closed")))
    }

    fn reconnect_client(&self) -> PyResult<()> {
        // These are intentionally two recovery layers with different
        // questions and ownership. Request scope in atm-http-runtime asks
        // whether this connection is stale for a still-configured endpoint:
        // it may perform one generation-aware, pre-send reconnect after
        // re-reading the endpoint record, decided solely by
        // `is_safe_to_reconnect()`. Session scope here asks whether the
        // daemon/endpoint itself is unavailable or caller identity/team
        // context must be re-established: it rebuilds the client under
        // `RefreshOnly` for the next call and never re-issues the failed
        // request. The `DaemonUnavailable` gate deliberately excludes
        // `WaitTimeout`; that boundary must not be widened.
        {
            let client = self
                .client
                .lock()
                .map_err(|_| poisoned_lock_error("ATM graft session"))?;
            if client.is_none() {
                return Err(atm_error(AtmError::daemon_unavailable(
                    "ATM graft session is closed",
                )));
            }
        }

        // Re-resolve the one selected daemon endpoint. A managed restart may
        // replace its Unix socket and invalidate a long-lived embedded host's
        // pooled transport; this never starts a competing daemon.
        #[cfg(test)]
        let replacement = {
            self.reconnect_attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.reconnect_replacement
                .lock()
                .map_err(|_| poisoned_lock_error("ATM graft reconnect"))?
                .clone()
                .map(Ok)
                .unwrap_or_else(|| {
                    self.reconnect_fallback_attempts
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    GraftClient::connect_existing().map_err(atm_error)
                })?
        };
        #[cfg(not(test))]
        let replacement = GraftClient::connect_existing().map_err(atm_error)?;
        let mut client = self
            .client
            .lock()
            .map_err(|_| poisoned_lock_error("ATM graft session"))?;
        if client.is_none() {
            return Err(atm_error(AtmError::daemon_unavailable(
                "ATM graft session is closed",
            )));
        }
        *client = Some(replacement);
        Ok(())
    }

    fn command_paths() -> PyResult<(std::path::PathBuf, std::path::PathBuf)> {
        Ok((
            atm_home().map_err(atm_error)?,
            command_invocation_dir().map_err(atm_error)?,
        ))
    }

    fn caller_team(&self) -> PyResult<TeamName> {
        self.caller.team().cloned().ok_or_else(|| {
            atm_error(AtmError::validation(
                "ATM graft caller identity requires a team",
            ))
        })
    }

    fn read_selection(selection: &str) -> PyResult<ReadSelection> {
        match selection {
            "actionable" => Ok(ReadSelection::Actionable),
            "all" => Ok(ReadSelection::All),
            "unread" => Ok(ReadSelection::Unread),
            "pending_ack" => Ok(ReadSelection::PendingAck),
            _ => Err(atm_error(AtmError::validation(
                "selection must be actionable, all, unread, or pending_ack",
            ))),
        }
    }

    fn send_outcome(
        &self,
        to: Option<String>,
        body: String,
        requires_ack: bool,
        acknowledges_message_id: Option<String>,
    ) -> PyResult<AtmSendResult> {
        if to.is_none() && acknowledges_message_id.is_none() {
            return Err(atm_error(AtmError::validation(
                "ATM send requires either a destination or acknowledges_message_id",
            )));
        }
        let (home_dir, current_dir) = Self::command_paths()?;
        let caller_team = self.caller_team()?;
        let acknowledgement = acknowledges_message_id
            .as_deref()
            .map(str::parse::<AtmMessageId>)
            .transpose()
            .map_err(|error| {
                atm_error(AtmError::validation(format!(
                    "invalid acknowledges_message_id: {error}"
                )))
            })?;
        let fallback_destination = self.caller.to_string();
        let request = SendRequest::new(
            home_dir,
            current_dir,
            self.caller.agent().clone(),
            to.as_deref().unwrap_or(&fallback_destination),
            caller_team.clone(),
            SendMessageSource::Inline(body),
            None,
            requires_ack,
            None,
            false,
        )
        .map_err(atm_error)?
        .with_caller_chat_id(self.caller.chat_id().cloned())
        .with_activity_observation(activity_observation_for_resolved_caller(
            self.caller.agent(),
            &caller_team,
        ));
        let request = match acknowledgement {
            Some(message_id) => request.with_acknowledges_message_id(message_id),
            None => request,
        };
        let client = self.client()?;
        python_extension_runtime()?
            .lock()
            .map_err(|_| poisoned_lock_error("ATM Python extension runtime"))?
            .block_on(client.write_message(request))
            .map(AtmSendResult::from)
            .map_err(atm_error)
    }

    fn emit_graft_event(
        &self,
        code: &'static str,
        fields: impl IntoIterator<Item = (&'static str, String)>,
    ) -> observability::ObservabilityStatus {
        self.fallback_logger.record(code, fields)
    }

    #[allow(clippy::result_large_err)]
    fn tool_error_from_recovery<T>(
        py: Python<'_>,
        policy: DaemonRecoveryPolicy,
        recovery: DaemonRecovery<T>,
    ) -> Result<(T, observability::ObservabilityStatus), AtmToolError> {
        match recovery {
            DaemonRecovery::Completed(value, status) => Ok((value, status)),
            DaemonRecovery::Failed {
                error,
                refreshed: true,
                refresh_error: None,
                observability,
            } if matches!(policy, DaemonRecoveryPolicy::RefreshOnly) => {
                Err(AtmToolError::from_native_error(py, &error).with_recovery(
                    "the native ATM session was refreshed after a transient failure; retry this send once. The failed send was not replayed to avoid duplicate delivery",
                ).with_observability(observability))
            }
            DaemonRecovery::Failed {
                error,
                refresh_error: Some(refresh_error),
                observability,
                ..
            } => Err(AtmToolError::from_native_error(py, &error).with_recovery(format!(
                "the native ATM session could not reconnect; verify the managed daemon is healthy, then retry ({refresh_error})"
            )).with_observability(observability)),
            DaemonRecovery::Failed { error, observability, .. } => {
                Err(AtmToolError::from_native_error(py, &error).with_observability(observability))
            }
        }
    }
}

#[pymethods]
impl PyGraftSession {
    #[new]
    fn new(caller: PyAgentAddress) -> PyResult<Self> {
        let (log_dir, _) = observability_log_dir()?;
        Ok(Self {
            caller: caller.to_typed()?,
            // A Python-embedded host must attach to the one runtime already
            // selected for this machine; it must never auto-start another one.
            client: Mutex::new(Some(GraftClient::connect_existing().map_err(atm_error)?)),
            receiver: Mutex::new(None),
            fallback_logger: observability::new_logger(&log_dir),
            #[cfg(test)]
            reconnect_replacement: Mutex::new(None),
            #[cfg(test)]
            reconnect_attempts: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            reconnect_fallback_attempts: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    #[pyo3(signature = (to, body, requires_ack=false))]
    fn send(
        &self,
        py: Python<'_>,
        to: PyAgentAddress,
        body: String,
        requires_ack: bool,
    ) -> PyResult<AtmSendResult> {
        let to = to.to_typed()?.to_string();
        // `detach` is PyO3 0.29's replacement for `allow_threads`: all
        // runtime blocking happens without holding the Python GIL.
        match self.with_daemon_recovery(py, DaemonRecoveryPolicy::RefreshOnly, "send", || {
            self.send_outcome(Some(to.clone()), body.clone(), requires_ack, None)
        }) {
            DaemonRecovery::Completed(outcome, status) => Ok(outcome.with_observability(status)),
            DaemonRecovery::Failed { error, .. } => Err(error),
        }
    }

    fn read(&self, py: Python<'_>) -> PyResult<Vec<PyMessage>> {
        let query = self.build_read_query(true)?;
        let outcome =
            match self.with_daemon_recovery(py, DaemonRecoveryPolicy::RetryOnce, "read", || {
                self.read_outcome(query.clone())
            }) {
                DaemonRecovery::Completed(outcome, status) => outcome.with_observability(status),
                DaemonRecovery::Failed { error, .. } => return Err(error),
            };
        outcome
            .message
            .map_or_else(|| Ok(Vec::new()), |message| Ok(vec![message]))
    }

    fn list(&self, py: Python<'_>) -> PyResult<usize> {
        let query = self.build_list_query("actionable", None, None, None, None, None)?;
        match self.with_daemon_recovery(py, DaemonRecoveryPolicy::RetryOnce, "list", || {
            self.list_outcome(query.clone())
                .map(|outcome| outcome.count_value())
        }) {
            DaemonRecovery::Completed(count, _) => Ok(count),
            DaemonRecovery::Failed { error, .. } => Err(error),
        }
    }

    /// Refresh the selected same-host client after a managed daemon cycle.
    ///
    /// This refreshes only this embedded session's transport; it never starts
    /// or changes the operator-owned daemon lifecycle.
    fn reconnect(&self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| self.reconnect_client())
    }

    /// Native-tool ingress delegates to the same canonical send implementation.
    #[pyo3(signature = (to, body, requires_ack=false, acknowledges_message_id=None))]
    fn send_tool(
        &self,
        py: Python<'_>,
        to: Option<String>,
        body: String,
        requires_ack: bool,
        acknowledges_message_id: Option<String>,
    ) -> PyResult<Py<PyAny>> {
        match Self::tool_error_from_recovery(
            py,
            DaemonRecoveryPolicy::RefreshOnly,
            self.with_daemon_recovery(
                py,
                DaemonRecoveryPolicy::RefreshOnly,
                if acknowledges_message_id.is_some() {
                    "ack"
                } else {
                    "send"
                },
                || {
                    self.send_outcome(
                        to.clone(),
                        body.clone(),
                        requires_ack,
                        acknowledges_message_id.clone(),
                    )
                },
            ),
        ) {
            Ok((outcome, status)) => {
                Ok(Py::new(py, outcome.with_observability(status))?.into_any())
            }
            Err(error) => Ok(Py::new(py, error)?.into_any()),
        }
    }

    /// Native acknowledgement tool; acknowledgements remain canonical sends
    /// carrying `acknowledges_message_id`.
    fn ack_tool(&self, py: Python<'_>, message_id: String, reply: String) -> PyResult<Py<PyAny>> {
        self.send_tool(py, None, reply, false, Some(message_id))
    }

    #[pyo3(signature = (selection="actionable", message_id=None, task=None, contains=None, since=None, from_agent=None))]
    #[allow(clippy::too_many_arguments)]
    fn read_tool(
        &self,
        py: Python<'_>,
        selection: &str,
        message_id: Option<String>,
        task: Option<String>,
        contains: Option<String>,
        since: Option<String>,
        from_agent: Option<String>,
    ) -> PyResult<Py<PyAny>> {
        let query = self.build_tool_read_query(
            selection,
            message_id.as_deref(),
            task.as_deref(),
            contains.as_deref(),
            since.as_deref(),
            from_agent.as_deref(),
        )?;
        match Self::tool_error_from_recovery(
            py,
            DaemonRecoveryPolicy::RetryOnce,
            self.with_daemon_recovery(py, DaemonRecoveryPolicy::RetryOnce, "read", || {
                self.peek_outcome(query.clone())
            }),
        ) {
            Ok((outcome, status)) => {
                Ok(Py::new(py, outcome.with_observability(status))?.into_any())
            }
            Err(error) => Ok(Py::new(py, error)?.into_any()),
        }
    }

    #[pyo3(signature = (selection="actionable", limit=None, task=None, contains=None, since=None, from_agent=None))]
    #[allow(clippy::too_many_arguments)]
    fn list_tool(
        &self,
        py: Python<'_>,
        selection: &str,
        limit: Option<usize>,
        task: Option<String>,
        contains: Option<String>,
        since: Option<String>,
        from_agent: Option<String>,
    ) -> PyResult<Py<PyAny>> {
        let query = self.build_list_query(
            selection,
            limit,
            task.as_deref(),
            contains.as_deref(),
            since.as_deref(),
            from_agent.as_deref(),
        )?;
        match Self::tool_error_from_recovery(
            py,
            DaemonRecoveryPolicy::RetryOnce,
            self.with_daemon_recovery(py, DaemonRecoveryPolicy::RetryOnce, "list", || {
                self.list_outcome(query.clone())
            }),
        ) {
            Ok((outcome, status)) => {
                Ok(Py::new(py, outcome.with_observability(status))?.into_any())
            }
            Err(error) => Ok(Py::new(py, error)?.into_any()),
        }
    }

    fn mailbox_work_counts(&self, py: Python<'_>) -> PyResult<PyMailboxWorkCounts> {
        let query = self.build_read_query(false)?;
        match self.with_daemon_recovery(py, DaemonRecoveryPolicy::RetryOnce, "read", || {
            self.read_raw(query.clone())
                .map(|outcome| PyMailboxWorkCounts {
                    unread: outcome.bucket_counts.unread,
                    pending_ack: outcome.bucket_counts.pending_ack,
                })
        }) {
            DaemonRecovery::Completed(counts, _) => Ok(counts),
            DaemonRecovery::Failed { error, .. } => Err(error),
        }
    }

    fn activate_receiver(
        &self,
        options: PyGraftSessionOptions,
        on_nudge: Py<PyAny>,
    ) -> PyResult<()> {
        let client = self.client()?;
        let mut receiver = self
            .receiver
            .lock()
            .map_err(|_| poisoned_lock_error("ATM graft receiver"))?;
        if receiver.is_some() {
            return Err(atm_error(AtmError::new(
                AtmErrorCode::GraftReceiverAlreadyActive,
                "ATM graft receiver is already active",
            )));
        }
        let session = client
            .activate_session(
                options
                    .to_typed()
                    .with_owner_chat_id(self.caller.chat_id().cloned()),
                Arc::new(PythonNudgeInjector { callback: on_nudge }),
            )
            .map_err(atm_error)?;
        *receiver = Some(session);
        Ok(())
    }

    fn snapshot(&self) -> PyResult<PyGraftSessionSnapshot> {
        let receiver = self
            .receiver
            .lock()
            .map_err(|_| poisoned_lock_error("ATM graft receiver"))?;
        receiver
            .as_ref()
            .ok_or_else(|| {
                atm_error(AtmError::daemon_unavailable(
                    "ATM graft receiver is not active",
                ))
            })?
            .snapshot()
            .map(PyGraftSessionSnapshot::from)
            .map_err(atm_error)
    }

    fn close(&self) -> PyResult<()> {
        let receiver = self
            .receiver
            .lock()
            .map_err(|_| poisoned_lock_error("ATM graft receiver"))?
            .take();
        if let Some(receiver) = receiver {
            receiver.close().map_err(atm_error)?;
        }
        self.client
            .lock()
            .map_err(|_| poisoned_lock_error("ATM graft session"))?
            .take();
        Ok(())
    }
}

#[pymodule]
fn _atm_graft(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("AtmGraftError", m.py().get_type::<AtmGraftError>())?;
    m.add_function(wrap_pyfunction!(observability_paths, m)?)?;
    m.add_class::<PyObservability>()?;
    m.add_class::<PyObservabilityPaths>()?;
    m.add_class::<PyAgentAddress>()?;
    m.add_class::<PyGraftSessionOptions>()?;
    m.add_class::<PyMessage>()?;
    m.add_class::<AtmSendResult>()?;
    m.add("AtmAckResult", m.py().get_type::<AtmSendResult>())?;
    m.add_class::<AtmReadResult>()?;
    m.add_class::<AtmListRow>()?;
    m.add_class::<AtmListResult>()?;
    m.add_class::<AtmToolError>()?;
    m.add_class::<PyNudge>()?;
    m.add_class::<PyGraftSessionSnapshot>()?;
    m.add_class::<PyMailboxWorkCounts>()?;
    m.add_class::<PyGraftSession>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        _atm_graft, AtmGraftError, AtmToolError, PyAgentAddress, PyGraftSession,
        PyGraftSessionOptions, PyMailboxWorkCounts, PyNudge, PythonNudgeInjector, atm_error,
        observability, observability_paths,
    };
    use atm_core::boundary::{NudgeKind, PostSendHookEvent};
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::list::ListOutcome;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
    use atm_core::read::{BucketCounts, ReadOutcome};
    use atm_core::send::{SendCommandOutcome, SendOutcome};
    use atm_core::test_support::EnvGuard;
    use atm_core::transport::testing::FakeClientTransport;
    use atm_core::types::{AgentName, ChatId, CommandAction, ReadSelection, TeamName};
    use atm_graft::{GraftClient, HostNudge, HostNudgeInjector, MailboxWorkCounts};
    use pyo3::prelude::{Py, Python};
    use pyo3::types::{PyAnyMethods, PyModule};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;
    use tempfile::TempDir;

    const TEST_RECIPIENT: &str = "test-recipient";
    const TEST_SENDER: &str = "test-sender";
    const TEST_TEAM: &str = "test-team";

    fn send_outcome() -> SendOutcome {
        SendOutcome {
            action: CommandAction::Send,
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_RECIPIENT),
            sender: AgentName::from_validated(TEST_SENDER),
            outcome: SendCommandOutcome::Sent,
            message_id: atm_core::schema::AtmMessageId::new(),
            requires_ack: false,
            task_id: None,
            summary: None,
            message: None,
            warnings: Vec::new(),
            dry_run: false,
        }
    }

    fn read_outcome() -> ReadOutcome {
        ReadOutcome {
            action: CommandAction::Read,
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_SENDER),
            selection_mode: ReadSelection::Actionable,
            mutation_applied: false,
            count: 0,
            message: None,
            selected_message_id: None,
            match_count: 0,
            additional_match_count: 0,
            bucket_counts: BucketCounts {
                unread: 2,
                pending_ack: 3,
                history: 5,
            },
        }
    }

    fn peek_outcome() -> ReadOutcome {
        ReadOutcome {
            action: CommandAction::Peek,
            ..read_outcome()
        }
    }

    fn list_outcome() -> ListOutcome {
        ListOutcome {
            action: CommandAction::List,
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_SENDER),
            selection_mode: ReadSelection::Actionable,
            history_collapsed: false,
            count: 0,
            rows: Vec::new(),
            bucket_counts: BucketCounts {
                unread: 2,
                pending_ack: 3,
                history: 5,
            },
        }
    }

    /// Returns a directory unique to this call for the fallback logger used
    /// by tests that don't assert on fallback-log content (see
    /// [`test_session_with_fallback_path`] for the path that does). A shared
    /// `std::env::temp_dir()` subdirectory would collide across parallel
    /// tests in this binary and across concurrent `cargo test` invocations on
    /// the same host; a fresh [`TempDir`] per call has no such collision.
    /// The directory is intentionally leaked (never removed) rather than
    /// tied to the caller's stack: the fallback logger's writer keeps
    /// appending to it for the life of the `PyGraftSession`, which in these
    /// tests is the whole test function, so there is no drop point earlier
    /// than process exit that would be safe to remove it at.
    fn leaked_fallback_dir() -> std::path::PathBuf {
        let tempdir = TempDir::new().expect("per-call fallback logger directory");
        let path = tempdir.path().to_path_buf();
        std::mem::forget(tempdir);
        path
    }

    fn test_session(
        initial: Arc<FakeClientTransport>,
        replacement: Arc<FakeClientTransport>,
    ) -> PyGraftSession {
        let caller = PyAgentAddress::new(TEST_SENDER.to_string(), TEST_TEAM.to_string(), None)
            .expect("caller");
        PyGraftSession {
            caller: caller.to_typed().expect("typed caller"),
            client: Mutex::new(Some(GraftClient::from_fake_transport_for_test(initial))),
            receiver: Mutex::new(None),
            fallback_logger: observability::new_logger(&leaked_fallback_dir()),
            reconnect_replacement: Mutex::new(Some(GraftClient::from_fake_transport_for_test(
                replacement,
            ))),
            reconnect_attempts: AtomicUsize::new(0),
            reconnect_fallback_attempts: AtomicUsize::new(0),
        }
    }

    fn test_session_with_fallback_path(
        initial: Arc<FakeClientTransport>,
        replacement: Arc<FakeClientTransport>,
        fallback_path: &std::path::Path,
        sender: String,
    ) -> PyGraftSession {
        let caller = PyAgentAddress::new(sender, TEST_TEAM.to_string(), None).expect("caller");
        PyGraftSession {
            caller: caller.to_typed().expect("typed caller"),
            client: Mutex::new(Some(GraftClient::from_fake_transport_for_test(initial))),
            receiver: Mutex::new(None),
            fallback_logger: Arc::new(observability::GraftFallbackLogger::new(
                fallback_path.to_owned(),
            )),
            reconnect_replacement: Mutex::new(Some(GraftClient::from_fake_transport_for_test(
                replacement,
            ))),
            reconnect_attempts: AtomicUsize::new(0),
            reconnect_fallback_attempts: AtomicUsize::new(0),
        }
    }

    #[test]
    fn four_concurrent_sessions_survive_restart_without_corrupt_fallback_lines() {
        Python::initialize();
        let tempdir = TempDir::new().expect("fallback log directory");
        let fallback_path = tempdir.path().join("atm-graft-fallback.jsonl");
        let initial_calls = Arc::new(AtomicUsize::new(0));
        let replacement_calls = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(4));
        let sessions = (0..4)
            .map(|index| {
                let initial_calls = Arc::clone(&initial_calls);
                let replacement_calls = Arc::clone(&replacement_calls);
                let initial = Arc::new(FakeClientTransport::new(Box::new(move |request| {
                    assert!(matches!(request, RequestEnvelope::List(_)));
                    initial_calls.fetch_add(1, Ordering::SeqCst);
                    Err(AtmError::daemon_unavailable("induced daemon restart"))
                })));
                let replacement = Arc::new(FakeClientTransport::new(Box::new(move |request| {
                    assert!(matches!(request, RequestEnvelope::List(_)));
                    replacement_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(ResponseEnvelope::List(list_outcome()))
                })));
                test_session_with_fallback_path(
                    initial,
                    replacement,
                    &fallback_path,
                    format!("{TEST_SENDER}-{index}"),
                )
            })
            .collect::<Vec<_>>();

        let outcomes = thread::scope(|scope| {
            sessions
                .into_iter()
                .map(|session| {
                    let start = Arc::clone(&start);
                    scope.spawn(move || {
                        start.wait();
                        Python::attach(|py| session.list(py).map_err(|error| error.to_string()))
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|handle| handle.join().expect("concurrent graft session"))
                .collect::<Result<Vec<_>, _>>()
        })
        .expect("all four primary outcomes survive the induced restart");

        assert_eq!(outcomes, vec![0; 4]);
        assert_eq!(initial_calls.load(Ordering::SeqCst), 4);
        assert_eq!(replacement_calls.load(Ordering::SeqCst), 4);

        let text = std::fs::read_to_string(&fallback_path).expect("fallback events");
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 12, "each recovered session emits three events");
        for line in &lines {
            assert!(line.starts_with("{\"origin\":\"graft\",\"ts\":\""));
            assert!(line.ends_with('}'), "event line is complete: {line}");
            assert!(line.contains("\"code\":\"ATM_GRAFT_"));
            assert!(line.contains("\"correlation_id\":\"graft-"));
        }
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("\"code\":\"ATM_GRAFT_RECOVERY_ATTEMPT\""))
                .count(),
            4
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("\"code\":\"ATM_GRAFT_DAEMON_UNAVAILABLE\""))
                .count(),
            4
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("\"code\":\"ATM_GRAFT_RECOVERY_RESULT\""))
                .count(),
            4
        );
    }

    #[test]
    fn observability_paths_reports_each_log_directory_source() {
        let tempdir = TempDir::new().expect("observability path fixtures");
        let log_dir = tempdir.path().join("explicit-logs");
        let atm_home = tempdir.path().join("atm-home");
        let log_dir_text = log_dir.to_str().expect("utf8 log path");
        let atm_home_text = atm_home.to_str().expect("utf8 ATM home");

        {
            let _env = EnvGuard::set_many([
                ("ATM_LOG_DIR", Some(log_dir_text)),
                ("ATM_HOME", Some(atm_home_text)),
            ]);
            let paths = observability_paths().expect("ATM_LOG_DIR paths");
            assert_eq!(paths.log_dir, log_dir.display().to_string());
            assert_eq!(paths.log_dir_source, "env:ATM_LOG_DIR");
            assert!(paths.canonical_log_path.starts_with(&paths.log_dir));
            assert!(paths.fallback_log_path.starts_with(&paths.log_dir));
        }

        {
            let _env =
                EnvGuard::set_many([("ATM_LOG_DIR", None), ("ATM_HOME", Some(atm_home_text))]);
            let paths = observability_paths().expect("ATM_HOME paths");
            let expected = atm_home.join(".atm").join("logs");
            assert_eq!(paths.log_dir, expected.display().to_string());
            assert_eq!(paths.log_dir_source, "env:ATM_HOME");
            assert!(paths.canonical_log_path.starts_with(&paths.log_dir));
            assert!(paths.fallback_log_path.starts_with(&paths.log_dir));
        }

        {
            let _env = EnvGuard::set_many([("ATM_LOG_DIR", None), ("ATM_HOME", None)]);
            let paths = observability_paths().expect("default paths");
            let expected = atm_core::home::host_log_dir()
                .expect("default host log directory")
                .display()
                .to_string();
            assert_eq!(paths.log_dir, expected);
            assert_eq!(paths.log_dir_source, "default");
            assert!(paths.canonical_log_path.starts_with(&paths.log_dir));
            assert!(paths.fallback_log_path.starts_with(&paths.log_dir));
        }
    }

    #[test]
    fn configured_fake_reconnect_does_not_open_the_live_client() {
        let initial = Arc::new(FakeClientTransport::new(Box::new(|_| {
            panic!("reconnect must not use the initial transport")
        })));
        let replacement = Arc::new(FakeClientTransport::new(Box::new(|_| {
            panic!("reconnect must not invoke the replacement transport")
        })));
        let session = test_session(initial, replacement);

        session
            .reconnect_client()
            .expect("configured fake replacement reconnects");

        assert_eq!(session.reconnect_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            session.reconnect_fallback_attempts.load(Ordering::SeqCst),
            0,
            "a configured fake must prevent any eager live-client connection"
        );
    }

    #[test]
    fn ffi_send_rejects_missing_destination_and_acknowledgement() {
        Python::initialize();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_transport = Arc::clone(&calls);
        let session = test_session(
            Arc::new(FakeClientTransport::new(Box::new(move |_| {
                calls_for_transport.fetch_add(1, Ordering::SeqCst);
                panic!("invalid FFI input must not reach the daemon client")
            }))),
            Arc::new(FakeClientTransport::new(Box::new(|_| {
                panic!("invalid FFI input must not reconnect")
            }))),
        );

        let error = session
            .send_outcome(None, "missing destination".to_owned(), false, None)
            .expect_err("FFI send must reject an operation without destination or acknowledgement");

        assert!(
            error
                .to_string()
                .contains("requires either a destination or acknowledges_message_id")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(session.reconnect_attempts.load(Ordering::SeqCst), 0);
    }

    fn host_nudge(event: PostSendHookEvent) -> HostNudge {
        HostNudge {
            kind: NudgeKind::Steer,
            body: "<atm><action>read atm</action></atm>".to_string(),
            notice_text: format!("📬 from {}\n{}", event.source_address(), event.description),
            event,
        }
    }

    #[test]
    fn typed_address_round_trips_optional_chat_id() {
        let address = PyAgentAddress::new(
            TEST_SENDER.to_string(),
            TEST_TEAM.to_string(),
            Some("1234".to_string()),
        )
        .expect("valid address");

        assert_eq!(
            address.to_typed().expect("typed address").to_string(),
            format!("{TEST_SENDER}:1234@{TEST_TEAM}")
        );
    }

    #[test]
    fn nudge_preserves_typed_source_chat_id() {
        let event = PostSendHookEvent {
            sender: AgentName::from_validated(TEST_SENDER),
            sender_chat_id: Some("1234".parse::<ChatId>().expect("chat id")),
            sender_team: TeamName::from_validated(TEST_TEAM),
            sender_host: None,
            recipient: AgentName::from_validated(TEST_RECIPIENT),
            recipient_team: TeamName::from_validated(TEST_TEAM),
            message_id: "01KX1TEST00000000000000000".parse().expect("message id"),
            description: "nudge".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id: None,
        };

        let nudge = PyNudge::from_post_send(&event).expect("python nudge");
        assert_eq!(nudge.source.chat_id.as_deref(), Some("1234"));
        assert_eq!(nudge.kind, "steer");
    }

    #[test]
    fn mailbox_work_counts_exposes_only_integer_count_fields_to_python() {
        Python::initialize();
        Python::attach(|py| {
            let counts = PyMailboxWorkCounts::from(MailboxWorkCounts {
                unread: 2,
                pending_ack: 3,
            });
            let object = Py::new(py, counts).expect("python count object");
            let object = object.bind(py);

            assert_eq!(
                object
                    .getattr("unread")
                    .expect("unread")
                    .extract::<usize>()
                    .unwrap(),
                2
            );
            assert_eq!(
                object
                    .getattr("pending_ack")
                    .expect("pending ack")
                    .extract::<usize>()
                    .unwrap(),
                3
            );
            assert!(object.getattr("body").is_err());
        });
    }

    #[test]
    fn python_session_exposes_typed_native_tools_and_acknowledgement() {
        Python::initialize();
        Python::attach(|py| {
            let module = PyModule::new(py, "_atm_graft").expect("python module");
            _atm_graft(&module).expect("register Python graft module");
            let session_type = module
                .getattr("PyGraftSession")
                .expect("Python graft session type");

            assert!(session_type.getattr("send").is_ok());
            assert!(session_type.getattr("read").is_ok());
            assert!(session_type.getattr("send_tool").is_ok());
            assert!(session_type.getattr("read_tool").is_ok());
            assert!(session_type.getattr("list_tool").is_ok());
            assert!(session_type.getattr("reconnect").is_ok());
            assert!(module.getattr("AtmSendResult").is_ok());
            assert!(module.getattr("AtmReadResult").is_ok());
            assert!(module.getattr("AtmListResult").is_ok());
            assert!(module.getattr("AtmAckResult").is_ok());
            assert!(module.getattr("PyObservability").is_ok());
            assert!(module.getattr("PyObservabilityPaths").is_ok());
            assert!(module.getattr("AtmToolError").is_ok());
            assert!(session_type.getattr("ack_tool").is_ok());
        });
    }

    #[test]
    fn closed_python_session_rejects_reconnect() {
        // reconnect_client converts the closed-session error into PyErr, so
        // this test must not rely on another parallel test initializing PyO3.
        Python::initialize();
        let caller = PyAgentAddress::new(TEST_SENDER.to_string(), TEST_TEAM.to_string(), None)
            .expect("caller");
        let session = PyGraftSession {
            caller: caller.to_typed().expect("typed caller"),
            client: Mutex::new(None),
            receiver: Mutex::new(None),
            fallback_logger: observability::new_logger(&leaked_fallback_dir()),
            reconnect_replacement: Mutex::new(None),
            reconnect_attempts: AtomicUsize::new(0),
            reconnect_fallback_attempts: AtomicUsize::new(0),
        };

        let error = session
            .reconnect_client()
            .expect_err("closed sessions must not reconnect");
        assert!(error.to_string().contains("session is closed"));
    }

    #[test]
    fn native_tool_selection_is_explicit_and_rejects_unknown_values() {
        assert_eq!(
            PyGraftSession::read_selection("actionable").expect("valid selection"),
            ReadSelection::Actionable
        );
        assert_eq!(
            PyGraftSession::read_selection("pending_ack").expect("valid selection"),
            ReadSelection::PendingAck
        );
        assert!(PyGraftSession::read_selection("mark_seen").is_err());
    }

    #[test]
    fn python_nudge_constructor_validates_immutable_event_fields() {
        let source = PyAgentAddress::new(
            TEST_SENDER.to_string(),
            TEST_TEAM.to_string(),
            Some("1234".to_string()),
        )
        .expect("valid source");

        let nudge = PyNudge::new(
            "01KX1TEST00000000000000000".to_string(),
            source,
            "nudge".to_string(),
            "steer",
        )
        .expect("valid nudge");
        assert_eq!(nudge.message_id, "01KX1TEST00000000000000000");
        assert_eq!(nudge.source.chat_id.as_deref(), Some("1234"));
        assert!(
            PyNudge::new(
                "not-a-ulid".to_string(),
                nudge.source,
                "nudge".to_string(),
                "steer",
            )
            .is_err()
        );
    }

    #[test]
    fn receiver_callback_receives_the_canonical_typed_nudge() {
        Python::initialize();
        let event = PostSendHookEvent {
            sender: AgentName::from_validated(TEST_SENDER),
            sender_chat_id: Some("1234".parse::<ChatId>().expect("chat id")),
            sender_team: TeamName::from_validated(TEST_TEAM),
            sender_host: None,
            recipient: AgentName::from_validated(TEST_RECIPIENT),
            recipient_team: TeamName::from_validated(TEST_TEAM),
            message_id: "01KX1TEST00000000000000000".parse().expect("message id"),
            description: "nudge".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id: None,
        };

        Python::attach(|py| {
            let module = PyModule::from_code(
                py,
                c"received = []\ndef callback(nudge):\n    assert nudge.source.chat_id == '1234'\n    received.append((nudge.message_id, nudge.body, nudge.notice_text))\n",
                c"receiver_callback_test.py",
                c"receiver_callback_test",
            )
            .expect("callback module");
            let injector = PythonNudgeInjector {
                callback: module.getattr("callback").expect("callback").unbind(),
            };

            injector
                .inject_nudge(&host_nudge(event))
                .expect("callback delivery");

            let received: Vec<(String, String, String)> = module
                .getattr("received")
                .expect("received")
                .extract()
                .expect("typed received nudges");
            assert_eq!(
                received,
                vec![(
                    "01KX1TEST00000000000000000".to_string(),
                    "<atm><action>read atm</action></atm>".to_string(),
                    "📬 from test-sender:1234@test-team\nnudge".to_string(),
                )]
            );
        });
    }

    #[test]
    fn receiver_callback_error_is_an_atm_error() {
        Python::initialize();
        let event = PostSendHookEvent {
            sender: AgentName::from_validated(TEST_SENDER),
            sender_chat_id: None,
            sender_team: TeamName::from_validated(TEST_TEAM),
            sender_host: None,
            recipient: AgentName::from_validated(TEST_RECIPIENT),
            recipient_team: TeamName::from_validated(TEST_TEAM),
            message_id: "01KX1TEST00000000000000000".parse().expect("message id"),
            description: "nudge".to_string(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
            recipient_pane_id: None,
        };

        Python::attach(|py| {
            let module = PyModule::from_code(
                py,
                c"def callback(_nudge):\n    raise RuntimeError('callback failed')\n",
                c"receiver_callback_error_test.py",
                c"receiver_callback_error_test",
            )
            .expect("callback module");
            let injector = PythonNudgeInjector {
                callback: module.getattr("callback").expect("callback").unbind(),
            };

            let error = injector
                .inject_nudge(&host_nudge(event))
                .expect_err("callback failure must propagate");
            assert_eq!(error.code(), atm_core::error::AtmErrorCode::InternalError);
            assert!(
                error
                    .cause()
                    .expect("Python callback cause must be retained")
                    .contains("callback failed")
            );
        });
    }

    #[test]
    fn invalid_address_returns_a_python_error() {
        Python::initialize();
        assert!(PyAgentAddress::new("bad:name".to_string(), TEST_TEAM.to_string(), None).is_err());
    }

    #[test]
    fn canonical_atm_errors_preserve_structured_fields_for_python() {
        Python::initialize();
        Python::attach(|py| {
            let error = atm_error(
                AtmError::daemon_unavailable("daemon socket is unavailable")
                    .with_cause("connection refused"),
            );
            let value = error.value(py);

            assert!(value.is_instance_of::<AtmGraftError>());
            assert_eq!(
                value
                    .getattr("code")
                    .expect("code field")
                    .extract::<String>()
                    .expect("string code"),
                "ATM_DAEMON_UNAVAILABLE"
            );
            assert!(
                value
                    .getattr("message")
                    .expect("message field")
                    .extract::<String>()
                    .expect("string message")
                    .starts_with("daemon socket is unavailable"),
                "the binding preserves the canonical message without owning its catalog text"
            );
            assert_eq!(
                value
                    .getattr("cause")
                    .expect("cause field")
                    .extract::<Option<String>>()
                    .expect("optional cause"),
                Some("connection refused".to_string())
            );
        });
    }

    #[test]
    fn native_tool_error_is_a_typed_python_result() {
        Python::initialize();
        let caller = PyAgentAddress::new(TEST_SENDER.to_string(), TEST_TEAM.to_string(), None)
            .expect("caller");
        let session = PyGraftSession {
            caller: caller.to_typed().expect("typed caller"),
            client: Mutex::new(None),
            receiver: Mutex::new(None),
            fallback_logger: observability::new_logger(&leaked_fallback_dir()),
            reconnect_replacement: Mutex::new(None),
            reconnect_attempts: AtomicUsize::new(0),
            reconnect_fallback_attempts: AtomicUsize::new(0),
        };

        Python::attach(|py| {
            let result = session
                .send_tool(
                    py,
                    Some(format!("{TEST_RECIPIENT}@{TEST_TEAM}")),
                    "typed native failure".to_owned(),
                    false,
                    None,
                )
                .expect("native tool returns a typed error object");
            let value = result.bind(py);

            assert!(value.is_instance_of::<AtmToolError>());
            assert_eq!(
                value
                    .getattr("code")
                    .expect("typed error code")
                    .extract::<String>()
                    .expect("string code"),
                "ATM_DAEMON_UNAVAILABLE"
            );
            assert_eq!(
                value
                    .getattr("layer")
                    .expect("typed error layer")
                    .extract::<String>()
                    .expect("string layer"),
                "native_client"
            );
        });
    }

    #[test]
    fn ack_tool_preserves_cli_code_for_a_non_pending_message() {
        Python::initialize();
        let message_id = "01KZSSREKYM7G39237P0YQ3CW3".to_owned();
        let expected_id = message_id.parse().expect("message id");
        let session = test_session(
            Arc::new(FakeClientTransport::new(Box::new(move |request| {
                let RequestEnvelope::Write(request) = request else {
                    panic!("ack must use the canonical write path")
                };
                assert_eq!(request.acknowledges_message_id, Some(expected_id));
                Err(AtmError::validation(
                    "acknowledgement source is not pending acknowledgement",
                ))
            }))),
            Arc::new(FakeClientTransport::new(Box::new(|_| {
                panic!("validation failure must not refresh the session")
            }))),
        );

        Python::attach(|py| {
            let result = session
                .ack_tool(py, message_id, "already handled".to_owned())
                .expect("ack returns a typed error result");
            let result = result.bind(py);
            assert!(result.is_instance_of::<AtmToolError>());
            assert_eq!(
                result
                    .getattr("code")
                    .expect("error code")
                    .extract::<String>()
                    .expect("string error code"),
                AtmErrorCode::MessageValidationFailed.as_str()
            );
            assert!(
                result
                    .getattr("message")
                    .expect("error message")
                    .extract::<String>()
                    .expect("string error message")
                    .contains("not pending acknowledgement")
            );
        });
        assert_eq!(session.reconnect_attempts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn send_tool_refreshes_once_without_replaying_a_failed_write() {
        Python::initialize();
        let initial_calls = Arc::new(AtomicUsize::new(0));
        let replacement_calls = Arc::new(AtomicUsize::new(0));
        let initial_calls_for_transport = Arc::clone(&initial_calls);
        let replacement_calls_for_transport = Arc::clone(&replacement_calls);
        let session = test_session(
            Arc::new(FakeClientTransport::new(Box::new(move |request| {
                assert!(matches!(request, RequestEnvelope::Write(_)));
                initial_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Err(AtmError::daemon_unavailable("test daemon cycle"))
            }))),
            Arc::new(FakeClientTransport::new(Box::new(move |request| {
                assert!(matches!(request, RequestEnvelope::Write(_)));
                replacement_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                    send_outcome(),
                )))
            }))),
        );

        Python::attach(|py| {
            let result = session
                .send_tool(
                    py,
                    Some(format!("{TEST_RECIPIENT}@{TEST_TEAM}")),
                    "do not replay".to_owned(),
                    false,
                    None,
                )
                .expect("typed recovery result");
            let result = result.bind(py);
            assert!(result.is_instance_of::<AtmToolError>());
            assert_eq!(
                result
                    .getattr("code")
                    .expect("code")
                    .extract::<String>()
                    .unwrap(),
                AtmErrorCode::DaemonUnavailable.as_str()
            );
            let recovery = result
                .getattr("recovery")
                .expect("recovery")
                .extract::<String>()
                .unwrap();
            assert!(recovery.contains("retry this send once"));
            assert!(recovery.contains("was not replayed"));
        });

        assert_eq!(initial_calls.load(Ordering::SeqCst), 1);
        assert_eq!(session.reconnect_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(replacement_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn send_tool_does_not_refresh_or_advise_resend_after_an_uncertain_write() {
        Python::initialize();
        let initial_calls = Arc::new(AtomicUsize::new(0));
        let replacement_calls = Arc::new(AtomicUsize::new(0));
        let initial_calls_for_transport = Arc::clone(&initial_calls);
        let replacement_calls_for_transport = Arc::clone(&replacement_calls);
        let session = test_session(
            Arc::new(FakeClientTransport::new(Box::new(move |request| {
                assert!(matches!(request, RequestEnvelope::Write(_)));
                initial_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Err(AtmError::daemon_may_have_executed(
                    "request write was interrupted",
                ))
            }))),
            Arc::new(FakeClientTransport::new(Box::new(move |request| {
                assert!(matches!(request, RequestEnvelope::Write(_)));
                replacement_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                    send_outcome(),
                )))
            }))),
        );

        Python::attach(|py| {
            let result = session
                .send_tool(
                    py,
                    Some(format!("{TEST_RECIPIENT}@{TEST_TEAM}")),
                    "do not resend an uncertain write".to_owned(),
                    false,
                    None,
                )
                .expect("typed uncertainty result");
            let result = result.bind(py);
            assert!(result.is_instance_of::<AtmToolError>());
            assert_eq!(
                result
                    .getattr("code")
                    .expect("code")
                    .extract::<String>()
                    .expect("string code"),
                AtmErrorCode::DaemonMayHaveExecuted.as_str()
            );
            let recovery = result
                .getattr("recovery")
                .expect("recovery")
                .extract::<String>()
                .expect("string recovery");
            assert!(recovery.contains("inspect mailbox"));
            assert!(!recovery.contains("retry this send"));
        });

        assert_eq!(initial_calls.load(Ordering::SeqCst), 1);
        assert_eq!(session.reconnect_attempts.load(Ordering::SeqCst), 0);
        assert_eq!(replacement_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn refresh_only_is_not_triggered_by_wait_timeout() {
        Python::initialize();
        let reconnect_attempts = Arc::new(AtomicUsize::new(0));
        let reconnect_attempts_for_transport = Arc::clone(&reconnect_attempts);
        let session = test_session(
            Arc::new(FakeClientTransport::new(Box::new(|request| {
                assert!(matches!(request, RequestEnvelope::Write(_)));
                Err(AtmError::new(
                    AtmErrorCode::WaitTimeout,
                    "request budget elapsed",
                ))
            }))),
            Arc::new(FakeClientTransport::new(Box::new(move |_| {
                reconnect_attempts_for_transport.fetch_add(1, Ordering::SeqCst);
                panic!("RefreshOnly must not reconnect after WaitTimeout")
            }))),
        );

        Python::attach(|py| {
            let result = session
                .send_tool(
                    py,
                    Some(format!("{TEST_RECIPIENT}@{TEST_TEAM}")),
                    "timeout is uncertain".to_owned(),
                    false,
                    None,
                )
                .expect("WaitTimeout is returned as a typed tool result");
            let result = result.bind(py);
            assert!(result.is_instance_of::<AtmToolError>());
            assert_eq!(
                result
                    .getattr("code")
                    .expect("code")
                    .extract::<String>()
                    .expect("string code"),
                AtmErrorCode::WaitTimeout.as_str()
            );
        });

        assert_eq!(session.reconnect_attempts.load(Ordering::SeqCst), 0);
        assert_eq!(reconnect_attempts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn plain_send_refreshes_once_without_replaying_a_failed_write() {
        Python::initialize();
        let initial_calls = Arc::new(AtomicUsize::new(0));
        let replacement_calls = Arc::new(AtomicUsize::new(0));
        let initial_calls_for_transport = Arc::clone(&initial_calls);
        let replacement_calls_for_transport = Arc::clone(&replacement_calls);
        let session = test_session(
            Arc::new(FakeClientTransport::new(Box::new(move |request| {
                assert!(matches!(request, RequestEnvelope::Write(_)));
                initial_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Err(AtmError::daemon_unavailable("test daemon cycle"))
            }))),
            Arc::new(FakeClientTransport::new(Box::new(move |request| {
                assert!(matches!(request, RequestEnvelope::Write(_)));
                replacement_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                    send_outcome(),
                )))
            }))),
        );
        let recipient =
            PyAgentAddress::new(TEST_RECIPIENT.to_string(), TEST_TEAM.to_string(), None)
                .expect("recipient");

        Python::attach(|py| {
            let error = session
                .send(py, recipient, "do not replay".to_owned(), false)
                .expect_err("plain write reports the potentially ambiguous failure");
            assert_eq!(
                AtmToolError::from_native_error(py, &error).code,
                AtmErrorCode::DaemonUnavailable.as_str()
            );
        });
        assert_eq!(initial_calls.load(Ordering::SeqCst), 1);
        assert_eq!(session.reconnect_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(replacement_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn native_read_and_list_retry_once_on_the_refreshed_fake_transport() {
        Python::initialize();
        for (operation, replacement_response) in [
            ("read", ResponseEnvelope::Peek(Box::new(peek_outcome()))),
            ("list", ResponseEnvelope::List(list_outcome())),
        ] {
            let initial_calls = Arc::new(AtomicUsize::new(0));
            let replacement_calls = Arc::new(AtomicUsize::new(0));
            let initial_calls_for_transport = Arc::clone(&initial_calls);
            let replacement_calls_for_transport = Arc::clone(&replacement_calls);
            let initial = Arc::new(FakeClientTransport::new(Box::new(move |request| {
                match operation {
                    "read" => assert!(matches!(request, RequestEnvelope::Peek(_))),
                    "list" => assert!(matches!(request, RequestEnvelope::List(_))),
                    _ => unreachable!(),
                }
                initial_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Err(AtmError::daemon_unavailable("test daemon cycle"))
            })));
            let replacement = Arc::new(FakeClientTransport::new(Box::new(move |request| {
                match operation {
                    "read" => assert!(matches!(request, RequestEnvelope::Peek(_))),
                    "list" => assert!(matches!(request, RequestEnvelope::List(_))),
                    _ => unreachable!(),
                }
                replacement_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Ok(replacement_response.clone())
            })));
            let session = test_session(initial, replacement);

            Python::attach(|py| {
                let result = match operation {
                    "read" => session.read_tool(py, "actionable", None, None, None, None, None),
                    "list" => session.list_tool(py, "actionable", None, None, None, None, None),
                    _ => unreachable!(),
                }
                .expect("read-only operation recovers");
                assert!(
                    !result.bind(py).is_instance_of::<AtmToolError>(),
                    "{operation} returns its success projection after one retry"
                );
                if operation == "read" {
                    let json = result
                        .bind(py)
                        .call_method0("to_json")
                        .expect("native read exposes canonical JSON")
                        .extract::<String>()
                        .expect("native read JSON is a string");
                    let value: serde_json::Value =
                        serde_json::from_str(&json).expect("native read JSON is valid");
                    assert_eq!(value["action"], "read");
                    assert!(value.get("additional_match_count").is_some());
                }
            });
            assert_eq!(initial_calls.load(Ordering::SeqCst), 1, "{operation}");
            assert_eq!(
                session.reconnect_attempts.load(Ordering::SeqCst),
                1,
                "{operation}"
            );
            assert_eq!(replacement_calls.load(Ordering::SeqCst), 1, "{operation}");
        }
    }

    #[test]
    fn native_read_scopes_peek_to_the_callers_chat_qualified_session() {
        let caller = PyAgentAddress::new(
            TEST_RECIPIENT.to_string(),
            TEST_TEAM.to_string(),
            Some("recipient-session".to_owned()),
        )
        .expect("caller");
        let session = PyGraftSession {
            caller: caller.to_typed().expect("typed caller"),
            client: Mutex::new(None),
            receiver: Mutex::new(None),
            fallback_logger: observability::new_logger(&leaked_fallback_dir()),
            reconnect_replacement: Mutex::new(None),
            reconnect_attempts: AtomicUsize::new(0),
            reconnect_fallback_attempts: AtomicUsize::new(0),
        };

        let query = session
            .build_tool_read_query("all", None, None, None, None, None)
            .expect("native read query");

        assert_eq!(
            query.caller_chat_id().map(ToString::to_string).as_deref(),
            Some("recipient-session")
        );
    }

    #[test]
    fn plain_python_methods_and_work_counts_share_the_recovery_policy() {
        Python::initialize();
        let operations: [(&str, ResponseEnvelope); 3] = [
            ("read", ResponseEnvelope::Receive(Box::new(read_outcome()))),
            ("list", ResponseEnvelope::List(list_outcome())),
            (
                "counts",
                ResponseEnvelope::Receive(Box::new(read_outcome())),
            ),
        ];
        for (operation, replacement_response) in operations {
            let initial_calls = Arc::new(AtomicUsize::new(0));
            let replacement_calls = Arc::new(AtomicUsize::new(0));
            let initial_calls_for_transport = Arc::clone(&initial_calls);
            let replacement_calls_for_transport = Arc::clone(&replacement_calls);
            let initial = Arc::new(FakeClientTransport::new(Box::new(move |request| {
                match operation {
                    "list" => assert!(matches!(request, RequestEnvelope::List(_))),
                    _ => assert!(matches!(request, RequestEnvelope::Receive(_))),
                }
                initial_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Err(AtmError::daemon_unavailable("test daemon cycle"))
            })));
            let replacement = Arc::new(FakeClientTransport::new(Box::new(move |request| {
                match operation {
                    "list" => assert!(matches!(request, RequestEnvelope::List(_))),
                    _ => assert!(matches!(request, RequestEnvelope::Receive(_))),
                }
                replacement_calls_for_transport.fetch_add(1, Ordering::SeqCst);
                Ok(replacement_response.clone())
            })));
            let session = test_session(initial, replacement);

            Python::attach(|py| match operation {
                "read" => assert!(session.read(py).expect("plain read recovery").is_empty()),
                "list" => assert_eq!(session.list(py).expect("plain list recovery"), 0),
                "counts" => {
                    let counts = session.mailbox_work_counts(py).expect("count recovery");
                    assert_eq!((counts.unread, counts.pending_ack), (2, 3));
                }
                _ => unreachable!(),
            });
            assert_eq!(initial_calls.load(Ordering::SeqCst), 1, "{operation}");
            assert_eq!(
                session.reconnect_attempts.load(Ordering::SeqCst),
                1,
                "{operation}"
            );
            assert_eq!(replacement_calls.load(Ordering::SeqCst), 1, "{operation}");
        }
    }

    #[test]
    fn python_send_uses_the_outer_ffi_runtime_bridge_for_the_async_client() {
        Python::initialize();
        let caller = PyAgentAddress::new(TEST_SENDER.to_string(), TEST_TEAM.to_string(), None)
            .expect("caller");
        let transport = Arc::new(FakeClientTransport::new(Box::new(|request| {
            assert!(matches!(
                request,
                atm_core::protocol::RequestEnvelope::Write(_)
            ));
            Ok(ResponseEnvelope::Send(SendResponseEnvelope::Sent(
                SendOutcome {
                    action: CommandAction::Send,
                    team: TeamName::from_validated(TEST_TEAM),
                    agent: AgentName::from_validated(TEST_RECIPIENT),
                    sender: AgentName::from_validated(TEST_SENDER),
                    outcome: SendCommandOutcome::Sent,
                    message_id: atm_core::schema::AtmMessageId::new(),
                    requires_ack: false,
                    task_id: None,
                    summary: None,
                    message: None,
                    warnings: Vec::new(),
                    dry_run: false,
                },
            )))
        })));
        let session = PyGraftSession {
            caller: caller.to_typed().expect("typed caller"),
            client: Mutex::new(Some(GraftClient::from_fake_transport_for_test(transport))),
            receiver: Mutex::new(None),
            fallback_logger: observability::new_logger(&leaked_fallback_dir()),
            reconnect_replacement: Mutex::new(None),
            reconnect_attempts: AtomicUsize::new(0),
            reconnect_fallback_attempts: AtomicUsize::new(0),
        };
        let recipient =
            PyAgentAddress::new(TEST_RECIPIENT.to_string(), TEST_TEAM.to_string(), None)
                .expect("recipient");

        Python::attach(|py| {
            session
                .send(py, recipient, "async through Python".to_owned(), false)
                .expect("Python boundary drives the asynchronous client");
        });
    }

    #[test]
    fn receiver_ownership_conflict_is_a_typed_python_error() {
        Python::initialize();
        Python::attach(|py| {
            let error = atm_error(AtmError::new(
                atm_core::error::AtmErrorCode::GraftReceiverAlreadyActive,
                "receiver already active for qa@test",
            ));
            let value = error.value(py);
            assert!(value.is_instance_of::<AtmGraftError>());
            assert_eq!(
                value
                    .getattr("code")
                    .expect("code field")
                    .extract::<String>()
                    .expect("string code"),
                "ATM_GRAFT_RECEIVER_ALREADY_ACTIVE"
            );
        });
    }

    #[test]
    fn python_session_rejects_a_second_receiver_activation() {
        Python::initialize();
        let tempdir = TempDir::new().expect("tempdir");
        let caller = PyAgentAddress::new(
            TEST_RECIPIENT.to_string(),
            TEST_TEAM.to_string(),
            Some("42".to_string()),
        )
        .expect("caller");
        let session = PyGraftSession {
            caller: caller.to_typed().expect("typed caller"),
            client: Mutex::new(Some(GraftClient::from_fake_transport_for_test(Arc::new(
                FakeClientTransport::new(Box::new(|request| match request {
                    RequestEnvelope::GraftReceiverRegister(_) => {
                        Ok(ResponseEnvelope::GraftReceiverRegister)
                    }
                    RequestEnvelope::GraftReceiverUnregister(_) => {
                        Ok(ResponseEnvelope::GraftReceiverUnregister)
                    }
                    _ => panic!("receiver activation sent an unexpected daemon request"),
                })),
            )))),
            receiver: Mutex::new(None),
            fallback_logger: observability::new_logger(&leaked_fallback_dir()),
            reconnect_replacement: Mutex::new(None),
            reconnect_attempts: AtomicUsize::new(0),
            reconnect_fallback_attempts: AtomicUsize::new(0),
        };
        let options = PyGraftSessionOptions::new(
            tempdir.path().display().to_string(),
            TEST_RECIPIENT.to_string(),
            TEST_TEAM.to_string(),
        )
        .expect("receiver options");

        Python::attach(|py| {
            let callback = PyModule::from_code(
                py,
                c"def callback(_nudge):\n    return None\n",
                c"receiver_activation_test.py",
                c"receiver_activation_test",
            )
            .expect("callback module")
            .getattr("callback")
            .expect("callback")
            .unbind();
            session
                .activate_receiver(options.clone(), callback.clone_ref(py))
                .expect("bare workspace receiver activation");
            assert_eq!(
                session.snapshot().expect("active snapshot").state,
                "listening"
            );
            let error = session
                .activate_receiver(options, callback)
                .expect_err("second activation must be rejected at the Python API boundary");
            assert!(error.is_instance_of::<AtmGraftError>(py));
            assert!(error.to_string().contains("already active"));
        });
    }
}
