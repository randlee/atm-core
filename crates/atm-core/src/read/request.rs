use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::address::{AgentAddress, MessageParticipantFilter, ParticipantDirection};
use crate::caller_context::ActivityObservation;
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::types::{AgentName, ChatId, IsoTimestamp, ReadSelection, TaskId, TeamName};

pub const MAX_CONTAINS_FILTER_LEN: usize = 1024;
pub const MAX_TIMEOUT_SECS: u64 = 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxQueryFields {
    pub(crate) home_dir: PathBuf,
    pub(crate) current_dir: PathBuf,
    pub(crate) target_address: Option<AgentAddress>,
    pub(crate) selection_mode: ReadSelection,
    pub(crate) seen_state_filter: bool,
    pub(crate) message_id_filter: Option<AtmMessageId>,
    pub(crate) sender_filter: Option<AgentName>,
    pub(crate) participant_filter: Option<MessageParticipantFilter>,
    pub(crate) timestamp_filter: Option<IsoTimestamp>,
    pub(crate) task_filter: Option<TaskId>,
    pub(crate) contains_filter: Option<String>,
    pub(crate) timeout_secs: Option<u64>,
}
impl MailboxQueryFields {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        target_address: Option<&str>,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        message_id_filter: Option<&str>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            home_dir,
            current_dir,
            target_address: target_address.map(str::parse).transpose()?,
            selection_mode,
            seen_state_filter,
            message_id_filter: message_id_filter
                .map(|value| {
                    value
                        .parse::<AtmMessageId>()
                        .map_err(|_| AtmError::validation(format!("invalid message id: {value}")))
                })
                .transpose()?,
            sender_filter: sender_filter.map(str::parse).transpose()?,
            participant_filter: None,
            timestamp_filter,
            task_filter: task_filter.map(str::parse).transpose()?,
            contains_filter: normalize_contains_filter(contains_filter)?,
            timeout_secs: validate_timeout_secs(timeout_secs)?,
        })
    }
}
#[allow(clippy::too_many_arguments)]
fn build_mailbox_query_fields(
    home_dir: PathBuf,
    current_dir: PathBuf,
    target_address: Option<&str>,
    selection_mode: ReadSelection,
    seen_state_filter: bool,
    message_id_filter: Option<&str>,
    sender_filter: Option<&str>,
    timestamp_filter: Option<IsoTimestamp>,
    task_filter: Option<&str>,
    contains_filter: Option<&str>,
    timeout_secs: Option<u64>,
) -> Result<MailboxQueryFields, AtmError> {
    MailboxQueryFields::new(
        home_dir,
        current_dir,
        target_address,
        selection_mode,
        seen_state_filter,
        message_id_filter,
        sender_filter,
        timestamp_filter,
        task_filter,
        contains_filter,
        timeout_secs,
    )
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeekQuery {
    pub(crate) mailbox: MailboxQueryFields,
    pub(crate) caller_identity: AgentName,
    pub(crate) caller_team: TeamName,
}
impl PeekQuery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        target_address: Option<&str>,
        caller_team: TeamName,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        message_id_filter: Option<&str>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, AtmError> {
        let mut mailbox = build_mailbox_query_fields(
            home_dir,
            current_dir,
            target_address,
            selection_mode,
            seen_state_filter,
            message_id_filter,
            sender_filter,
            timestamp_filter,
            task_filter,
            contains_filter,
            timeout_secs,
        )?;
        mailbox.participant_filter = Some(MessageParticipantFilter {
            agent: caller_identity.clone(),
            chat_id: None,
            direction: ParticipantDirection::To,
        });
        Ok(Self {
            mailbox,
            caller_identity,
            caller_team,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadQuery {
    pub(crate) mailbox: MailboxQueryFields,
    pub(crate) caller_identity: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) caller_chat_id: Option<ChatId>,
    pub(crate) caller_team: TeamName,
    pub(crate) seen_state_update: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_observation: Option<ActivityObservation>,
}
impl ReadQuery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        target_address: Option<&str>,
        caller_team: TeamName,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        seen_state_update: bool,
        message_id_filter: Option<&str>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            mailbox: build_mailbox_query_fields(
                home_dir,
                current_dir,
                target_address,
                selection_mode,
                seen_state_filter,
                message_id_filter,
                sender_filter,
                timestamp_filter,
                task_filter,
                contains_filter,
                timeout_secs,
            )?,
            caller_identity,
            caller_chat_id: None,
            caller_team,
            seen_state_update,
            activity_observation: None,
        })
    }
    pub fn team_override(&self) -> Option<&TeamName> {
        Some(&self.caller_team)
    }
    pub fn selection_mode(&self) -> ReadSelection {
        self.mailbox.selection_mode
    }
    pub fn seen_state_filter(&self) -> bool {
        self.mailbox.seen_state_filter
    }
    pub fn seen_state_update(&self) -> bool {
        self.seen_state_update
    }
    pub fn message_id_filter(&self) -> Option<&AtmMessageId> {
        self.mailbox.message_id_filter.as_ref()
    }
    pub fn timeout_secs(&self) -> Option<u64> {
        self.mailbox.timeout_secs
    }
    pub fn caller_chat_id(&self) -> Option<&ChatId> {
        self.caller_chat_id.as_ref()
    }
    #[must_use]
    pub fn with_caller_chat_id(mut self, caller_chat_id: Option<ChatId>) -> Self {
        self.mailbox.participant_filter = Some(MessageParticipantFilter {
            agent: self.caller_identity.clone(),
            chat_id: caller_chat_id.clone(),
            direction: ParticipantDirection::To,
        });
        self.caller_chat_id = caller_chat_id;
        self
    }
    #[must_use]
    pub fn with_activity_observation(
        mut self,
        activity_observation: Option<ActivityObservation>,
    ) -> Self {
        self.activity_observation = activity_observation;
        self
    }
    pub fn with_selection_mode(mut self, selection_mode: ReadSelection) -> Self {
        self.mailbox.selection_mode = selection_mode;
        self
    }
}
pub(crate) fn normalize_contains_filter(
    contains_filter: Option<&str>,
) -> Result<Option<String>, AtmError> {
    match contains_filter
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) if value.len() > MAX_CONTAINS_FILTER_LEN => Err(AtmError::validation(format!(
            "contains filter exceeds the {}-byte maximum",
            MAX_CONTAINS_FILTER_LEN
        ))),
        Some(value) => Ok(Some(value.to_ascii_lowercase())),
        None => Ok(None),
    }
}
fn validate_timeout_secs(timeout_secs: Option<u64>) -> Result<Option<u64>, AtmError> {
    match timeout_secs {
        Some(value) if value > MAX_TIMEOUT_SECS => Err(AtmError::validation(format!(
            "timeout exceeds the {} second maximum",
            MAX_TIMEOUT_SECS
        ))),
        _ => Ok(timeout_secs),
    }
}
