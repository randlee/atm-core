use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::address::{AgentAddress, MessageParticipantFilter, ParticipantDirection};
use crate::caller_context::ActivityObservation;
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::types::{AgentName, ChatId, IsoTimestamp, ReadSelection, TaskId, TeamName};

pub const MAX_CONTAINS_FILTER_LEN: usize = 1024;
pub const MAX_TIMEOUT_SECS: u64 = 3600;

/// Named query filters for read and peek requests.
///
/// This builder keeps the string-backed filters distinct at the call site, so
/// a sender, task, message ID, and body-match value cannot be accidentally
/// transposed by a positional constructor call.
#[derive(Debug, Clone, Default)]
pub struct MailboxQueryFilters {
    target_address: Option<String>,
    message_id: Option<String>,
    sender: Option<String>,
    timestamp: Option<IsoTimestamp>,
    task: Option<String>,
    contains: Option<String>,
    timeout_secs: Option<u64>,
}

impl MailboxQueryFilters {
    #[must_use]
    pub fn target_address_opt(mut self, value: Option<&str>) -> Self {
        self.target_address = value.map(str::to_owned);
        self
    }

    #[must_use]
    pub fn target_address(mut self, value: impl Into<String>) -> Self {
        self.target_address = Some(value.into());
        self
    }

    #[must_use]
    pub fn message_id(mut self, value: impl Into<String>) -> Self {
        self.message_id = Some(value.into());
        self
    }

    #[must_use]
    pub fn message_id_opt(mut self, value: Option<&str>) -> Self {
        self.message_id = value.map(str::to_owned);
        self
    }

    #[must_use]
    pub fn sender(mut self, value: impl Into<String>) -> Self {
        self.sender = Some(value.into());
        self
    }

    #[must_use]
    pub fn sender_opt(mut self, value: Option<&str>) -> Self {
        self.sender = value.map(str::to_owned);
        self
    }

    #[must_use]
    pub fn timestamp(mut self, value: IsoTimestamp) -> Self {
        self.timestamp = Some(value);
        self
    }

    #[must_use]
    pub const fn timestamp_opt(mut self, value: Option<IsoTimestamp>) -> Self {
        self.timestamp = value;
        self
    }

    #[must_use]
    pub fn task(mut self, value: impl Into<String>) -> Self {
        self.task = Some(value.into());
        self
    }

    #[must_use]
    pub fn task_opt(mut self, value: Option<&str>) -> Self {
        self.task = value.map(str::to_owned);
        self
    }

    #[must_use]
    pub fn contains(mut self, value: impl Into<String>) -> Self {
        self.contains = Some(value.into());
        self
    }

    #[must_use]
    pub fn contains_opt(mut self, value: Option<&str>) -> Self {
        self.contains = value.map(str::to_owned);
        self
    }

    #[must_use]
    pub const fn timeout_secs(mut self, value: u64) -> Self {
        self.timeout_secs = Some(value);
        self
    }

    #[must_use]
    pub const fn timeout_secs_opt(mut self, value: Option<u64>) -> Self {
        self.timeout_secs = value;
        self
    }
}

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
    fn with_daemon_paths(mut self, daemon_home: PathBuf) -> Self {
        self.home_dir = daemon_home.clone();
        self.current_dir = daemon_home;
        self
    }

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeekQuery {
    pub(crate) mailbox: MailboxQueryFields,
    pub(crate) caller_identity: AgentName,
    pub(crate) caller_team: TeamName,
}
impl PeekQuery {
    /// Replaces caller-supplied filesystem roots with the daemon-owned root
    /// before a request crosses the long-lived service boundary.
    #[must_use]
    pub fn with_daemon_paths(mut self, daemon_home: PathBuf) -> Self {
        self.mailbox = self.mailbox.with_daemon_paths(daemon_home);
        self
    }

    pub fn from_filters(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        caller_team: TeamName,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        filters: MailboxQueryFilters,
    ) -> Result<Self, AtmError> {
        Self::new(
            home_dir,
            current_dir,
            caller_identity,
            filters.target_address.as_deref(),
            caller_team,
            selection_mode,
            seen_state_filter,
            filters.message_id.as_deref(),
            filters.sender.as_deref(),
            filters.timestamp,
            filters.task.as_deref(),
            filters.contains.as_deref(),
            filters.timeout_secs,
        )
    }

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
        let mut mailbox = MailboxQueryFields::new(
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
    /// Replaces caller-supplied filesystem roots with the daemon-owned root
    /// before a request crosses the long-lived service boundary.
    #[must_use]
    pub fn with_daemon_paths(mut self, daemon_home: PathBuf) -> Self {
        self.mailbox = self.mailbox.with_daemon_paths(daemon_home);
        self
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the request boundary keeps caller, selection, and mutation state explicit; all optional filters are named in MailboxQueryFilters"
    )]
    pub fn from_filters(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        caller_team: TeamName,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        seen_state_update: bool,
        filters: MailboxQueryFilters,
    ) -> Result<Self, AtmError> {
        Self::new(
            home_dir,
            current_dir,
            caller_identity,
            filters.target_address.as_deref(),
            caller_team,
            selection_mode,
            seen_state_filter,
            seen_state_update,
            filters.message_id.as_deref(),
            filters.sender.as_deref(),
            filters.timestamp,
            filters.task.as_deref(),
            filters.contains.as_deref(),
            filters.timeout_secs,
        )
    }

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
            mailbox: MailboxQueryFields::new(
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
