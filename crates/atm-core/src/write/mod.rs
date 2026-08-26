//! The one canonical mail-write pipeline.
//!
//! This module owns durable write admission for both sends and
//! acknowledgements: entry points, the shared [`PreparedWrite`] hand-off, and
//! acknowledgement admission. `send` re-exports the public entry points and
//! `ack` re-exports the acknowledgement request/outcome types, so external
//! paths (including serde/persisted shapes) are unchanged.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[cfg(test)]
use crate::boundary::MessageReceivedHookEmitter;
use crate::boundary::{self, BuiltInPostSendDispatch};
use crate::caller_context::ActivityObservation;
use crate::delivery_policy::DeliveryPolicyCoordinator;
use crate::error::AtmError;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::provenance::{
    ValidatedWriteProvenance, WriteIngress, WriteProvenance, validate_write_provenance,
};
use crate::schema::{AtmMessageId, InboxMessage, authenticated_source_host, peer_delivery_target};
use crate::send::{
    DeliveryExecutionMode, DuplicateWriteDisposition, PreparedReceivedHook, ResolvedRecipient,
    SendCommandOutcome, SendMessageSource, SendOutcome, SendRequest, WriteRequest,
    annotate_path_only_body, emit_send_command_event, finalize_send_outcome, persist_send_message,
    prepare_received_hook, prepare_send_context, request_requires_ack, resolve_message_body,
};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, ChatId, CommandAction, HostName, IsoTimestamp, TaskId, TeamName};
use atm_storage::contract::{
    AcknowledgementReplyBuilder, AcknowledgementSource, Message as StoredMessage, MessageKey,
};

/// Parameters for acknowledging one pending-ack mailbox message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_chat_id: Option<ChatId>,
    pub caller_team: TeamName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_observation: Option<ActivityObservation>,
    pub message_id: AtmMessageId,
    pub reply_body: String,
}

impl AckRequest {
    pub fn into_write_request(self) -> SendRequest {
        SendRequest {
            home_dir: self.home_dir,
            current_dir: self.current_dir,
            caller_identity: self.caller_identity,
            caller_chat_id: self.caller_chat_id,
            caller_team: self.caller_team,
            activity_observation: self.activity_observation,
            authenticated_source_host: None,
            origin_message_id: None,
            origin_timestamp: None,
            to: None,
            message_source: SendMessageSource::Inline(self.reply_body),
            classification: crate::send::MessageClassification::default(),
            max_message_bytes: crate::send::input::default_message_max_bytes(),
            summary_override: None,
            requires_ack: false,
            task_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            acknowledges_message_id: Some(self.message_id),
            dry_run: false,
            nudge_mode: crate::send::NudgeMode::default(),
        }
    }

    pub fn from_unresolved_write(request: SendRequest) -> Result<Self, AtmError> {
        let message_id = request.acknowledges_message_id.ok_or_else(|| {
            AtmError::validation("acknowledgement write is missing acknowledges_message_id")
        })?;
        let reply_body = match request.message_source {
            SendMessageSource::Inline(reply_body) => reply_body,
            SendMessageSource::File { .. } | SendMessageSource::Template(_) => {
                return Err(AtmError::validation(
                    "acknowledgement reply body must be inline",
                ));
            }
        };
        Ok(Self {
            home_dir: request.home_dir,
            current_dir: request.current_dir,
            caller_identity: request.caller_identity,
            caller_chat_id: request.caller_chat_id,
            caller_team: request.caller_team,
            activity_observation: request.activity_observation,
            message_id,
            reply_body,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AckReplyDisposition {
    Sent {
        reply_message_id: AtmMessageId,
        reply_target: ReplyTarget,
    },
}

/// Summary of one successful acknowledgement and reply handling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub message_id: AtmMessageId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub reply_disposition: AckReplyDisposition,
    pub reply_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<crate::send::WarningEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    agent: AgentName,
    team: TeamName,
    host: Option<HostName>,
}

impl ReplyTarget {
    pub(crate) fn new(agent: AgentName, team: TeamName, host: Option<HostName>) -> Self {
        Self { agent, team, host }
    }
}

impl std::fmt::Display for ReplyTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.agent, self.team)?;
        if let Some(host) = &self.host {
            write!(f, ".{host}")?;
        }
        Ok(())
    }
}

impl Serialize for ReplyTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ReplyTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let address: crate::address::AgentAddress =
            value.parse().map_err(serde::de::Error::custom)?;
        let team = address
            .team()
            .cloned()
            .ok_or_else(|| serde::de::Error::custom("expected <agent>@<team> reply target"))?;
        Ok(Self::new(
            address.agent().clone(),
            team,
            address.host().cloned(),
        ))
    }
}

mod acknowledgement;
mod pipeline;

pub use pipeline::{
    PreparedWrite, WriteOutcome, prepare_write_with_async_runtime, prepare_write_with_runtime,
    send_mail, send_mail_with_runtime, write_mail, write_mail_with_runtime,
};

#[cfg(test)]
pub(crate) use pipeline::{send_mail_with_runtime_impl, write_mail_with_runtime_impl};

pub(crate) use acknowledgement::{
    AtomicAcknowledgementWrite, ResolvedAcknowledgement, admit_acknowledgement_write,
    admit_acknowledgement_write_async,
};
#[cfg(test)]
pub(crate) use acknowledgement::{
    build_atomic_acknowledgement, canonical_ack_write_request, reply_target_host,
};
