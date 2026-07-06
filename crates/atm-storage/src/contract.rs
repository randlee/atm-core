use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use crate::error::AtmError;
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::types::{AgentName, IsoTimestamp, ModelName, PaneId, TaskId, TeamName};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct MessageKey(String);

impl MessageKey {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("message key must not be blank").with_recovery(
                    "Populate a stable ATM message key before calling the storage contract.",
                ),
            );
        }
        Ok(Self(value))
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_atm_message_id(&self) -> Result<AtmMessageId, AtmError> {
        let raw = self.as_str().strip_prefix("atm:").unwrap_or(self.as_str());
        raw.parse::<AtmMessageId>()
            .map_err(|error| AtmError::validation(format!("message key parse failed: {error}")))
    }

    fn from_atm_message_id(value: AtmMessageId) -> Self {
        let mut key = String::with_capacity(4 + value.to_string().len());
        key.push_str("atm:");
        key.push_str(&value.to_string());
        Self(key)
    }
}

impl From<AtmMessageId> for MessageKey {
    fn from(value: AtmMessageId) -> Self {
        Self::from_atm_message_id(value)
    }
}

impl FromStr for MessageKey {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Display for MessageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for MessageKey {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct TaskState(String);

impl TaskState {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("task state must not be blank").with_recovery(
                    "Populate a non-empty task state before calling the storage contract.",
                ),
            );
        }
        Ok(Self(value))
    }
}

impl AsRef<str> for TaskState {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Deref for TaskState {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for TaskState {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl PartialEq<&str> for TaskState {
    fn eq(&self, other: &&str) -> bool {
        self.as_ref() == *other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AckTransition(String);

impl AckTransition {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(
                AtmError::validation("ack transition must not be blank").with_recovery(
                    "Populate a non-empty ack transition before calling the storage contract.",
                ),
            );
        }
        Ok(Self(value))
    }
}

impl AsRef<str> for AckTransition {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Deref for AckTransition {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for AckTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for AckTransition {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub envelope: MessageEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailMessageState {
    pub team: TeamName,
    pub agent: AgentName,
    pub actor: AgentName,
    pub message_key: MessageKey,
    pub read: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_ack_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<IsoTimestamp>,
}

/// Opaque hash or content-addressable identifier that marks the last
/// successfully ingested message boundary for a replay source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageFingerprint(String);

impl MessageFingerprint {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for MessageFingerprint {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for MessageFingerprint {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageQuery {
    pub team: TeamName,
    pub agent: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<AgentName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RosterMemberKind {
    Permanent,
    Ephemeral,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RosterHarness {
    ClaudeCode,
    CodexCli,
    GeminiCli,
    Opencode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentType {
    GeneralPurpose,
    Plan,
    Lead,
    Qa,
    Worker,
    Unknown(String),
}

impl Default for AgentType {
    fn default() -> Self {
        Self::Unknown(String::new())
    }
}

impl From<String> for AgentType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "general-purpose" => Self::GeneralPurpose,
            "plan" => Self::Plan,
            "lead" => Self::Lead,
            "qa" => Self::Qa,
            "worker" => Self::Worker,
            _ => Self::Unknown(value),
        }
    }
}

impl AgentType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::GeneralPurpose => "general-purpose",
            Self::Plan => "plan",
            Self::Lead => "lead",
            Self::Qa => "qa",
            Self::Worker => "worker",
            Self::Unknown(value) => value,
        }
    }
}

impl From<AgentType> for String {
    fn from(value: AgentType) -> Self {
        value.as_str().to_string()
    }
}

impl Serialize for AgentType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from(String::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterMember {
    pub team_name: TeamName,
    pub agent_name: AgentName,
    pub member_kind: RosterMemberKind,
    pub harness: RosterHarness,
    #[serde(default)]
    pub agent_type: AgentType,
    #[serde(default)]
    pub model: ModelName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_pane_id: Option<PaneId>,
    #[serde(default)]
    pub metadata_json: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterSnapshot {
    pub team_name: TeamName,
    pub members: Vec<RosterMember>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<IsoTimestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageReceivedEvent {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
    pub timestamp: IsoTimestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterChangedEvent {
    pub team: TeamName,
    pub member_count: usize,
    pub timestamp: IsoTimestamp,
}

pub trait MessageStore {
    fn save_message(&self, message: &Message) -> Result<(), AtmError>;
    fn load_message(&self, key: &MessageKey) -> Result<Option<Message>, AtmError>;
    fn list_messages(&self, query: &MessageQuery) -> Result<Vec<Message>, AtmError>;
    fn delete_message(&self, key: &MessageKey) -> Result<(), AtmError>;
}

pub trait RosterStore {
    fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError>;
    fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError>;
    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError>;
}

pub trait StorageNotifier {
    fn message_received(&self, event: &MessageReceivedEvent) -> Result<(), AtmError>;
    fn roster_changed(&self, event: &RosterChangedEvent) -> Result<(), AtmError>;
}

#[cfg(test)]
mod tests {
    use super::{
        Message, MessageKey, MessageQuery, MessageReceivedEvent, MessageStore, RosterChangedEvent,
        RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot, RosterStore,
        StorageNotifier,
    };
    use crate::ROLE_WORKER;
    use crate::error::AtmError;
    use crate::schema::MessageEnvelope;
    use crate::types::{AgentName, IsoTimestamp, ModelName, TeamName};
    use chrono::Utc;
    use serde_json::Map;

    #[derive(Default)]
    struct DummyStore;

    impl MessageStore for DummyStore {
        fn save_message(&self, _message: &Message) -> Result<(), AtmError> {
            Ok(())
        }

        fn load_message(&self, _key: &MessageKey) -> Result<Option<Message>, AtmError> {
            Ok(None)
        }

        fn list_messages(&self, _query: &MessageQuery) -> Result<Vec<Message>, AtmError> {
            Ok(Vec::new())
        }

        fn delete_message(&self, _key: &MessageKey) -> Result<(), AtmError> {
            Ok(())
        }
    }

    impl RosterStore for DummyStore {
        fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError> {
            Ok(RosterSnapshot {
                team_name: team.clone(),
                members: Vec::new(),
                refreshed_at: None,
            })
        }

        fn save_roster(&self, _roster: &RosterSnapshot) -> Result<(), AtmError> {
            Ok(())
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            Ok(Vec::new())
        }
    }

    impl StorageNotifier for DummyStore {
        fn message_received(&self, _event: &MessageReceivedEvent) -> Result<(), AtmError> {
            Ok(())
        }

        fn roster_changed(&self, _event: &RosterChangedEvent) -> Result<(), AtmError> {
            Ok(())
        }
    }

    #[test]
    fn storage_traits_are_object_safe() {
        let store = DummyStore;
        let message_store: &dyn MessageStore = &store;
        let roster_store: &dyn RosterStore = &store;
        let notifier: &dyn StorageNotifier = &store;

        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = ROLE_WORKER.parse().expect("agent");
        let key = MessageKey::new("atm:test-1").expect("key");

        let message = Message {
            team: team.clone(),
            agent: agent.clone(),
            message_key: key.clone(),
            envelope: MessageEnvelope {
                from: agent.clone(),
                text: "hello".to_string(),
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
                read: false,
                source_team: Some(team.clone()),
                summary: None,
                message_id: None,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            },
        };
        let roster = RosterSnapshot {
            team_name: team.clone(),
            members: vec![RosterMember {
                team_name: team.clone(),
                agent_name: agent.clone(),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: super::AgentType::Worker,
                model: ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            }],
            refreshed_at: None,
        };

        message_store.save_message(&message).expect("save message");
        assert!(message_store.load_message(&key).expect("load").is_none());
        assert!(
            message_store
                .list_messages(&MessageQuery {
                    team: team.clone(),
                    agent: agent.clone(),
                    sender: None,
                    task_id: None,
                    limit: Some(5),
                })
                .expect("list")
                .is_empty()
        );
        message_store.delete_message(&key).expect("delete");

        roster_store.save_roster(&roster).expect("save roster");
        assert_eq!(
            roster_store
                .load_roster(&team)
                .expect("load roster")
                .team_name,
            team
        );
        assert!(roster_store.list_teams().expect("list teams").is_empty());

        notifier
            .message_received(&MessageReceivedEvent {
                team: team.clone(),
                agent: agent.clone(),
                message_key: key,
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
            })
            .expect("message notification");
        notifier
            .roster_changed(&RosterChangedEvent {
                team,
                member_count: 1,
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
            })
            .expect("roster notification");
    }
}
