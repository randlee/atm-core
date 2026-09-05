//! Full Python projection of one canonical classified mailbox message.

use atm_core::address::AgentAddress;
use atm_core::read::ReadOutcome;
use atm_core::schema::ThreadMode;
use pyo3::prelude::*;

use super::{PyAgentAddress, atm_error};
use crate::tool_types::canonical_timestamp;

#[pyclass(skip_from_py_object)]
#[derive(Clone, Debug)]
pub struct PyMessage {
    #[pyo3(get)]
    pub(crate) message_id: Option<String>,
    #[pyo3(get)]
    pub(crate) source: PyAgentAddress,
    #[pyo3(get)]
    pub(crate) body: String,
    #[pyo3(get)]
    pub(crate) bucket: String,
    message_class: String,
    #[pyo3(get)]
    pub(crate) text: String,
    #[pyo3(get)]
    pub(crate) timestamp: String,
    #[pyo3(get)]
    pub(crate) read: bool,
    #[pyo3(get)]
    pub(crate) source_team: Option<String>,
    #[pyo3(get)]
    pub(crate) source_chat_id: Option<String>,
    #[pyo3(get)]
    pub(crate) destination_chat_id: Option<String>,
    #[pyo3(get)]
    pub(crate) summary: Option<String>,
    #[pyo3(get)]
    pub(crate) requires_ack: bool,
    #[pyo3(get)]
    pub(crate) pending_ack_at: Option<String>,
    #[pyo3(get)]
    pub(crate) acknowledged_at: Option<String>,
    #[pyo3(get)]
    pub(crate) acknowledges_message_id: Option<String>,
    #[pyo3(get)]
    pub(crate) parent_message_id: Option<String>,
    #[pyo3(get)]
    pub(crate) thread_mode: Option<String>,
    #[pyo3(get)]
    pub(crate) expires_at: Option<String>,
    #[pyo3(get)]
    pub(crate) task_id: Option<String>,
}

impl PyMessage {
    pub(crate) fn from_read(outcome: ReadOutcome) -> PyResult<Vec<Self>> {
        outcome
            .message
            .map(|message| {
                let envelope = message.envelope;
                PyAgentAddress::from_typed(
                    AgentAddress::new(
                        envelope.from.clone(),
                        envelope.source_chat_id.clone(),
                        envelope.source_team.clone(),
                        None,
                    )
                    .map_err(atm_error)?,
                )
                .map(|source| Self {
                    message_id: envelope.message_id.map(|id| id.to_string()),
                    source,
                    body: envelope.text.clone(),
                    bucket: match message.bucket {
                        atm_core::types::DisplayBucket::Unread => "unread",
                        atm_core::types::DisplayBucket::PendingAck => "pending_ack",
                        atm_core::types::DisplayBucket::History => "history",
                    }
                    .to_owned(),
                    message_class: match message.class {
                        atm_core::types::MessageClass::Unread => "unread",
                        atm_core::types::MessageClass::PendingAck => "pending_ack",
                        atm_core::types::MessageClass::Acknowledged => "acknowledged",
                        atm_core::types::MessageClass::Read => "read",
                    }
                    .to_owned(),
                    text: envelope.text,
                    timestamp: canonical_timestamp(&envelope.timestamp),
                    read: envelope.read,
                    source_team: envelope.source_team.map(|team| team.to_string()),
                    source_chat_id: envelope.source_chat_id.map(|id| id.to_string()),
                    destination_chat_id: envelope.destination_chat_id.map(|id| id.to_string()),
                    summary: envelope.summary,
                    requires_ack: envelope.requires_ack,
                    pending_ack_at: envelope.pending_ack_at.as_ref().map(canonical_timestamp),
                    acknowledged_at: envelope.acknowledged_at.as_ref().map(canonical_timestamp),
                    acknowledges_message_id: envelope
                        .acknowledges_message_id
                        .map(|id| id.to_string()),
                    parent_message_id: envelope.parent_message_id.map(|id| id.to_string()),
                    thread_mode: envelope.thread_mode.map(|mode| match mode {
                        ThreadMode::AddDetails => "add-details".to_owned(),
                        ThreadMode::Supersede => "supersede".to_owned(),
                    }),
                    expires_at: envelope.expires_at.as_ref().map(canonical_timestamp),
                    task_id: envelope.task_id.map(|id| id.to_string()),
                })
            })
            .transpose()
            .map(Option::into_iter)
            .map(Iterator::collect)
    }
}

#[pymethods]
impl PyMessage {
    #[getter(class)]
    fn class_name(&self) -> String {
        self.message_class.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "PyMessage(message_id={:?}, source={})",
            self.message_id, self.source.agent
        )
    }
}
