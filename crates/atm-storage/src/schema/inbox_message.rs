use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use ulid::Ulid;

use crate::types::{AgentName, ChatId, IsoTimestamp, TaskId, TeamName};

#[derive(Debug, Clone)]
pub struct AtmMessageIdParseError(String);

impl fmt::Display for AtmMessageIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AtmMessageIdParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AtmMessageId(Ulid);

impl AtmMessageId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn into_ulid(self) -> Ulid {
        self.0
    }

    pub fn timestamp(self) -> IsoTimestamp {
        let datetime: DateTime<Utc> = self.0.datetime().into();
        IsoTimestamp::from_datetime(datetime)
    }

    pub fn new_with_timestamp() -> (Self, IsoTimestamp) {
        let message_id = Self::new();
        let timestamp = message_id.timestamp();
        (message_id, timestamp)
    }
}

impl Default for AtmMessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Ulid> for AtmMessageId {
    fn from(value: Ulid) -> Self {
        Self(value)
    }
}

impl From<AtmMessageId> for Ulid {
    fn from(value: AtmMessageId) -> Self {
        value.0
    }
}

impl std::str::FromStr for AtmMessageId {
    type Err = AtmMessageIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_string(s)
            .map(Self)
            .map_err(|error| AtmMessageIdParseError(error.to_string()))
    }
}

impl fmt::Display for AtmMessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreadMode {
    AddDetails,
    Supersede,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertKind {
    MissingTeamConfig,
    Unknown(String),
}

impl AlertKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::MissingTeamConfig => "missing_team_config",
            Self::Unknown(value) => value,
        }
    }
}

impl From<String> for AlertKind {
    fn from(value: String) -> Self {
        match value.as_str() {
            "missing_team_config" => Self::MissingTeamConfig,
            _ => Self::Unknown(value),
        }
    }
}

impl From<AlertKind> for String {
    fn from(value: AlertKind) -> Self {
        match value {
            AlertKind::MissingTeamConfig => "missing_team_config".to_string(),
            AlertKind::Unknown(value) => value,
        }
    }
}

impl Serialize for AlertKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AlertKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from(String::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MessageEnvelope {
    pub from: AgentName,
    /// Optional source context, persisted independently of `from` so agent
    /// names never encode chat identity.
    #[serde(
        rename = "sourceChatId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub source_chat_id: Option<ChatId>,
    pub text: String,
    pub timestamp: IsoTimestamp,
    pub read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_team: Option<TeamName>,
    /// Optional destination context, retained with the immutable message
    /// envelope for exact reply and acknowledgement targeting.
    #[serde(
        rename = "destinationChatId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub destination_chat_id: Option<ChatId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<AtmMessageId>,
    pub requires_ack: bool,
    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,
    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,
    #[serde(
        rename = "acknowledgesMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub acknowledges_message_id: Option<AtmMessageId>,
    #[serde(
        rename = "parentMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_message_id: Option<AtmMessageId>,
    #[serde(rename = "threadMode", skip_serializing_if = "Option::is_none")]
    pub thread_mode: Option<ThreadMode>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<IsoTimestamp>,
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(
        rename = "taskComplete",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub task_complete: Option<TaskId>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct RawMessageEnvelope {
    from: AgentName,
    #[serde(
        rename = "sourceChatId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    source_chat_id: Option<ChatId>,
    text: String,
    timestamp: IsoTimestamp,
    read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_team: Option<TeamName>,
    #[serde(
        rename = "destinationChatId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    destination_chat_id: Option<ChatId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message_id: Option<AtmMessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    requires_ack: Option<bool>,
    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pending_ack_at: Option<IsoTimestamp>,
    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    acknowledged_at: Option<IsoTimestamp>,
    #[serde(
        rename = "acknowledgesMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    acknowledges_message_id: Option<AtmMessageId>,
    #[serde(
        rename = "parentMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    parent_message_id: Option<AtmMessageId>,
    #[serde(rename = "threadMode", skip_serializing_if = "Option::is_none")]
    thread_mode: Option<ThreadMode>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    expires_at: Option<IsoTimestamp>,
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    task_id: Option<TaskId>,
    #[serde(
        rename = "taskComplete",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    task_complete: Option<TaskId>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl From<RawMessageEnvelope> for MessageEnvelope {
    fn from(value: RawMessageEnvelope) -> Self {
        let requires_ack = value
            .requires_ack
            .unwrap_or(value.pending_ack_at.is_some() && value.acknowledges_message_id.is_none());
        Self {
            from: value.from,
            source_chat_id: value.source_chat_id,
            text: value.text,
            timestamp: value.timestamp,
            read: value.read,
            source_team: value.source_team,
            destination_chat_id: value.destination_chat_id,
            summary: value.summary,
            message_id: value.message_id,
            requires_ack,
            pending_ack_at: value.pending_ack_at,
            acknowledged_at: value.acknowledged_at,
            acknowledges_message_id: value.acknowledges_message_id,
            parent_message_id: value.parent_message_id,
            thread_mode: value.thread_mode,
            expires_at: value.expires_at,
            task_id: value.task_id,
            task_complete: value.task_complete,
            extra: value.extra,
        }
    }
}

impl<'de> Deserialize<'de> for MessageEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawMessageEnvelope::deserialize(deserializer).map(Into::into)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingAck {
    pub message_id: AtmMessageId,
    pub from: AgentName,
    pub acked: bool,
    pub acked_at: Option<IsoTimestamp>,
}

#[cfg(test)]
mod tests {
    use super::MessageEnvelope;
    use crate::types::{AgentName, IsoTimestamp, TaskId};

    fn envelope(task_complete: Option<TaskId>) -> MessageEnvelope {
        MessageEnvelope {
            from: "sender".parse::<AgentName>().expect("agent"),
            source_chat_id: None,
            text: "task update".to_owned(),
            timestamp: "2026-09-04T00:00:00Z"
                .parse::<IsoTimestamp>()
                .expect("time"),
            read: false,
            source_team: None,
            destination_chat_id: None,
            summary: None,
            message_id: None,
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            task_complete,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn task_complete_round_trips_and_omits_absent_carrier() {
        let absent = serde_json::to_value(envelope(None)).expect("serialize");
        assert!(absent.get("taskComplete").is_none());
        let decoded: MessageEnvelope = serde_json::from_value(absent).expect("deserialize");
        assert_eq!(decoded.task_complete, None);

        let task: TaskId = "AX.3".parse().expect("task");
        let present = serde_json::to_value(envelope(Some(task.clone()))).expect("serialize");
        assert_eq!(
            present.get("taskComplete").and_then(|value| value.as_str()),
            Some(task.as_str())
        );
        let decoded: MessageEnvelope = serde_json::from_value(present).expect("deserialize");
        assert_eq!(decoded.task_complete, Some(task));
    }
}
