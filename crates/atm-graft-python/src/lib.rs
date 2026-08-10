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
use atm_core::ack::AckRequest;
use atm_core::address::AgentAddress;
use atm_core::boundary::PostSendHookEvent;
use atm_core::caller_context::activity_observation_for_resolved_caller;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::graft::AtmGraftClient;
use atm_core::home::{atm_home, command_invocation_dir};
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::types::{AgentName, ChatId, ReadSelection, TeamName};
use pyo3::exceptions::{PyException, PyRuntimeError, PyValueError};
use pyo3::prelude::*;

fn python_extension_runtime() -> PyResult<&'static Mutex<tokio::runtime::Runtime>> {
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
            PyRuntimeError::new_err(format!(
                "failed to create the ATM Python extension runtime: {error}"
            ))
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
            .ok_or_else(|| PyValueError::new_err("ATM address requires a team"))?;
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
}

impl PyGraftSessionOptions {
    fn to_typed(&self) -> PyResult<GraftSessionOptions> {
        if self.workspace_root.trim().is_empty() {
            return Err(PyValueError::new_err("workspace_root must not be blank"));
        }
        Ok(GraftSessionOptions::new(
            &self.workspace_root,
            self.team.parse::<TeamName>().map_err(atm_error)?,
            self.agent.parse::<AgentName>().map_err(atm_error)?,
        ))
    }
}

#[pymethods]
impl PyGraftSessionOptions {
    #[new]
    fn new(workspace_root: String, agent: String, team: String) -> PyResult<Self> {
        let options = Self {
            workspace_root,
            agent,
            team,
        };
        options.to_typed()?;
        Ok(options)
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

#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct PyMessage {
    #[pyo3(get)]
    message_id: Option<String>,
    #[pyo3(get)]
    source: PyAgentAddress,
    #[pyo3(get)]
    body: String,
}

impl PyMessage {
    fn from_read(outcome: ReadOutcome) -> PyResult<Vec<Self>> {
        outcome
            .message
            .map(|message| {
                PyAgentAddress::from_typed(
                    AgentAddress::new(
                        message.envelope.from,
                        message.envelope.source_chat_id,
                        message.envelope.source_team,
                        None,
                    )
                    .map_err(atm_error)?,
                )
                .map(|source| Self {
                    message_id: message.envelope.message_id.map(|id| id.to_string()),
                    source,
                    body: message.envelope.text,
                })
            })
            .transpose()
            .map(Option::into_iter)
            .map(Iterator::collect)
    }
}

#[pymethods]
impl PyMessage {
    fn __repr__(&self) -> String {
        format!(
            "PyMessage(message_id={:?}, source={})",
            self.message_id, self.source.agent
        )
    }
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
}

impl PyNudge {
    pub fn from_post_send(event: &PostSendHookEvent) -> PyResult<Self> {
        let body = event.description.clone();
        Ok(Self {
            message_id: event.message_id.to_string(),
            source: PyAgentAddress::from_typed(event.source_address())?,
            notice_text: body.clone(),
            body,
        })
    }

    pub fn from_host_nudge(nudge: &HostNudge) -> PyResult<Self> {
        Ok(Self {
            message_id: nudge.event.message_id.to_string(),
            source: PyAgentAddress::from_typed(nudge.event.source_address())?,
            body: nudge.body.clone(),
            notice_text: nudge.notice_text.clone(),
        })
    }
}

#[pymethods]
impl PyNudge {
    #[new]
    fn new(message_id: String, source: PyAgentAddress, body: String) -> PyResult<Self> {
        message_id
            .parse::<atm_core::schema::AtmMessageId>()
            .map_err(|error| PyValueError::new_err(format!("invalid message id: {error}")))?;
        if body.trim().is_empty() {
            return Err(PyValueError::new_err("nudge body must not be blank"));
        }
        Ok(Self {
            message_id,
            source,
            notice_text: body.clone(),
            body,
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
}

impl PyGraftSession {
    fn client(&self) -> PyResult<GraftClient> {
        self.client
            .lock()
            .map_err(|_| PyRuntimeError::new_err("ATM graft session lock poisoned"))?
            .clone()
            .ok_or_else(|| PyRuntimeError::new_err("ATM graft session is closed"))
    }

    fn command_paths() -> PyResult<(std::path::PathBuf, std::path::PathBuf)> {
        Ok((
            atm_home().map_err(atm_error)?,
            command_invocation_dir().map_err(atm_error)?,
        ))
    }

    fn caller_team(&self) -> PyResult<TeamName> {
        self.caller
            .team()
            .cloned()
            .ok_or_else(|| PyRuntimeError::new_err("ATM graft caller identity requires a team"))
    }

    fn build_read_query(&self, seen_state_update: bool) -> PyResult<ReadQuery> {
        let (home_dir, current_dir) = Self::command_paths()?;
        let team = self.caller_team()?;
        let query = ReadQuery::new(
            home_dir,
            current_dir,
            self.caller.agent().clone(),
            None,
            team.clone(),
            ReadSelection::All,
            false,
            seen_state_update,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .map_err(atm_error)?;
        Ok(query
            .with_caller_chat_id(self.caller.chat_id().cloned())
            .with_activity_observation(activity_observation_for_resolved_caller(
                self.caller.agent(),
                &team,
            )))
    }
}

#[pymethods]
impl PyGraftSession {
    #[new]
    fn new(caller: PyAgentAddress) -> PyResult<Self> {
        Ok(Self {
            caller: caller.to_typed()?,
            client: Mutex::new(Some(GraftClient::connect().map_err(atm_error)?)),
            receiver: Mutex::new(None),
        })
    }

    #[pyo3(signature = (to, body, requires_ack=false))]
    fn send(&self, to: PyAgentAddress, body: String, requires_ack: bool) -> PyResult<()> {
        let (home_dir, current_dir) = Self::command_paths()?;
        let caller_team = self.caller_team()?;
        let request = SendRequest::new(
            home_dir,
            current_dir,
            self.caller.agent().clone(),
            &to.to_typed()?.to_string(),
            caller_team.clone(),
            SendMessageSource::Inline(body),
            None,
            requires_ack,
            None,
            false,
        )
        .map_err(atm_error)?
        .with_caller_chat_id(self.caller.chat_id().cloned());
        let request = request.with_activity_observation(activity_observation_for_resolved_caller(
            self.caller.agent(),
            &caller_team,
        ));
        let client = self.client()?;
        // Python calls are synchronous. This is the one approved runtime
        // bridge at the outermost PyO3 boundary; library clients never bridge
        // async work back to synchronous code.
        python_extension_runtime()?
            .lock()
            .map_err(|_| PyRuntimeError::new_err("ATM Python extension runtime lock poisoned"))?
            .block_on(client.send_message(request))
            .map_err(atm_error)?;
        Ok(())
    }

    fn read(&self) -> PyResult<Vec<PyMessage>> {
        let query = self.build_read_query(true)?;
        PyMessage::from_read(self.client()?.read_message(query).map_err(atm_error)?)
    }

    fn mailbox_work_counts(&self) -> PyResult<PyMailboxWorkCounts> {
        let query = self.build_read_query(false)?;
        self.client()?
            .mailbox_work_counts(query)
            .map(PyMailboxWorkCounts::from)
            .map_err(atm_error)
    }

    fn acknowledge(&self, message_id: String, reply_body: String) -> PyResult<()> {
        let (home_dir, current_dir) = Self::command_paths()?;
        let caller_team = self.caller_team()?;
        let request = AckRequest {
            home_dir,
            current_dir,
            caller_identity: self.caller.agent().clone(),
            caller_chat_id: self.caller.chat_id().cloned(),
            caller_team: caller_team.clone(),
            activity_observation: activity_observation_for_resolved_caller(
                self.caller.agent(),
                &caller_team,
            ),
            message_id: message_id
                .parse()
                .map_err(|error| PyValueError::new_err(format!("invalid message id: {error}")))?,
            reply_body,
        };
        self.client()?
            .acknowledge_message(request)
            .map_err(atm_error)?;
        Ok(())
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
            .map_err(|_| PyRuntimeError::new_err("ATM graft receiver lock poisoned"))?;
        if receiver.is_some() {
            return Err(PyRuntimeError::new_err(
                "ATM graft receiver is already active",
            ));
        }
        let session = client
            .activate_session(
                options
                    .to_typed()?
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
            .map_err(|_| PyRuntimeError::new_err("ATM graft receiver lock poisoned"))?;
        receiver
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("ATM graft receiver is not active"))?
            .snapshot()
            .map(PyGraftSessionSnapshot::from)
            .map_err(atm_error)
    }

    fn close(&self) -> PyResult<()> {
        let receiver = self
            .receiver
            .lock()
            .map_err(|_| PyRuntimeError::new_err("ATM graft receiver lock poisoned"))?
            .take();
        if let Some(receiver) = receiver {
            receiver.close().map_err(atm_error)?;
        }
        self.client
            .lock()
            .map_err(|_| PyRuntimeError::new_err("ATM graft session lock poisoned"))?
            .take();
        Ok(())
    }
}

#[pymodule]
fn _atm_graft(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("AtmGraftError", m.py().get_type::<AtmGraftError>())?;
    m.add_class::<PyAgentAddress>()?;
    m.add_class::<PyGraftSessionOptions>()?;
    m.add_class::<PyMessage>()?;
    m.add_class::<PyNudge>()?;
    m.add_class::<PyGraftSessionSnapshot>()?;
    m.add_class::<PyMailboxWorkCounts>()?;
    m.add_class::<PyGraftSession>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AtmGraftError, PyAgentAddress, PyGraftSession, PyGraftSessionOptions, PyMailboxWorkCounts,
        PyNudge, PythonNudgeInjector, atm_error,
    };
    use atm_core::boundary::PostSendHookEvent;
    use atm_core::error::AtmError;
    use atm_core::protocol::{ResponseEnvelope, SendResponseEnvelope};
    use atm_core::send::{SendCommandOutcome, SendOutcome};
    use atm_core::transport::testing::FakeClientTransport;
    use atm_core::types::{AgentName, ChatId, CommandAction, TeamName};
    use atm_graft::{GraftClient, HostNudge, HostNudgeInjector, MailboxWorkCounts};
    use pyo3::prelude::{Py, Python};
    use pyo3::types::{PyAnyMethods, PyModule};
    use std::fs;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    const TEST_RECIPIENT: &str = "test-recipient";
    const TEST_SENDER: &str = "test-sender";
    const TEST_TEAM: &str = "test-team";

    fn host_nudge(event: PostSendHookEvent) -> HostNudge {
        HostNudge {
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
        )
        .expect("valid nudge");
        assert_eq!(nudge.message_id, "01KX1TEST00000000000000000");
        assert_eq!(nudge.source.chat_id.as_deref(), Some("1234"));
        assert!(PyNudge::new("not-a-ulid".to_string(), nudge.source, "nudge".to_string()).is_err());
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
        };
        let recipient =
            PyAgentAddress::new(TEST_RECIPIENT.to_string(), TEST_TEAM.to_string(), None)
                .expect("recipient");

        Python::attach(|_| {
            session
                .send(recipient, "async through Python".to_owned(), false)
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
        fs::write(
            tempdir.path().join(".atm.toml"),
            "[graft]\nenabled = true\n",
        )
        .expect("graft config");
        let caller = PyAgentAddress::new(
            TEST_RECIPIENT.to_string(),
            TEST_TEAM.to_string(),
            Some("42".to_string()),
        )
        .expect("caller");
        let session = PyGraftSession {
            caller: caller.to_typed().expect("typed caller"),
            client: Mutex::new(Some(GraftClient::from_fake_transport_for_test(Arc::new(
                FakeClientTransport::new(Box::new(|_| {
                    panic!("receiver activation must not call the daemon transport")
                })),
            )))),
            receiver: Mutex::new(None),
        };
        let options = PyGraftSessionOptions {
            workspace_root: tempdir.path().display().to_string(),
            agent: TEST_RECIPIENT.to_string(),
            team: TEST_TEAM.to_string(),
        };

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
                .expect("first receiver activation");
            let error = session
                .activate_receiver(options, callback)
                .expect_err("second activation must be rejected at the Python API boundary");
            assert!(error.is_instance_of::<pyo3::exceptions::PyRuntimeError>(py));
            assert!(error.to_string().contains("already active"));
        });
    }
}
