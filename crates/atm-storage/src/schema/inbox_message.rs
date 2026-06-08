use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use ulid::Ulid;
use uuid::Uuid;

use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

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

    pub fn from_uuid_wire(value: Uuid) -> Self {
        Self(Ulid::from_bytes(value.into_bytes()))
    }

    pub fn into_uuid_wire(self) -> Uuid {
        Uuid::from_bytes(self.0.to_bytes())
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

impl From<Uuid> for AtmMessageId {
    fn from(value: Uuid) -> Self {
        Self::from_uuid_wire(value)
    }
}

impl From<AtmMessageId> for Ulid {
    fn from(value: AtmMessageId) -> Self {
        value.0
    }
}

impl From<AtmMessageId> for Uuid {
    fn from(value: AtmMessageId) -> Self {
        value.into_uuid_wire()
    }
}

impl std::str::FromStr for AtmMessageId {
    type Err = AtmMessageIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_string(s)
            .map(Self)
            .or_else(|_| Uuid::parse_str(s).map(Self::from_uuid_wire))
            .map_err(|error| AtmMessageIdParseError(error.to_string()))
    }
}

impl fmt::Display for AtmMessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

mod atm_message_id_uuid_wire {
    use super::AtmMessageId;
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S>(value: &AtmMessageId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.into_uuid_wire().to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AtmMessageId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Uuid::parse_str(&raw)
            .map(AtmMessageId::from_uuid_wire)
            .map_err(serde::de::Error::custom)
    }
}

mod option_atm_message_id_uuid_wire {
    use super::AtmMessageId;
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S>(value: &Option<AtmMessageId>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer.serialize_some(&value.into_uuid_wire().to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<AtmMessageId>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer)?
            .map(|raw| {
                let uuid = Uuid::parse_str(&raw).map_err(serde::de::Error::custom)?;
                Ok(AtmMessageId::from_uuid_wire(uuid))
            })
            .transpose()
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageEnvelope {
    pub from: AgentName,
    pub text: String,
    pub timestamp: IsoTimestamp,
    pub read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_team: Option<TeamName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "option_atm_message_id_uuid_wire")]
    pub message_id: Option<AtmMessageId>,
    #[serde(rename = "pendingAckAt", skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,
    #[serde(rename = "acknowledgedAt", skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,
    #[serde(
        rename = "acknowledgesMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[serde(with = "option_atm_message_id_uuid_wire")]
    pub acknowledges_message_id: Option<AtmMessageId>,
    #[serde(
        rename = "parentMessageId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[serde(with = "option_atm_message_id_uuid_wire")]
    pub parent_message_id: Option<AtmMessageId>,
    #[serde(rename = "threadMode", skip_serializing_if = "Option::is_none")]
    pub thread_mode: Option<ThreadMode>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<IsoTimestamp>,
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingAck {
    #[serde(with = "atm_message_id_uuid_wire")]
    pub message_id: AtmMessageId,
    pub from: AgentName,
    pub acked: bool,
    pub acked_at: Option<IsoTimestamp>,
}

