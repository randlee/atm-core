use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

use crate::address::validate_path_segment;
use crate::error::AtmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IsoTimestamp(DateTime<Utc>);

impl IsoTimestamp {
    pub fn now() -> Self {
        Self(Utc::now())
    }

    pub fn from_datetime(datetime: DateTime<Utc>) -> Self {
        Self(datetime)
    }

    pub fn into_inner(self) -> DateTime<Utc> {
        self.0
    }
}

impl From<DateTime<Utc>> for IsoTimestamp {
    fn from(datetime: DateTime<Utc>) -> Self {
        Self(datetime)
    }
}

/// Canonical ATM agent/member name at a public API boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentName(String);

impl AgentName {
    /// Borrow the wrapped agent name as `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the inner owned name.
    pub fn into_inner(self) -> String {
        self.0
    }

    pub(crate) fn from_validated(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl FromStr for AgentName {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        validate_path_segment(trimmed, "agent")?;
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for AgentName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl From<AgentName> for String {
    fn from(value: AgentName) -> Self {
        value.0
    }
}

impl AsRef<str> for AgentName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for AgentName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for AgentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for AgentName {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// Opaque external Claude Code `agentId` value carried across roster/config
/// boundaries without ATM reinterpreting its internal format.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<AgentId> for String {
    fn from(value: AgentId) -> Self {
        value.0
    }
}

impl AsRef<str> for AgentId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for AgentId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical ATM team name at a public API boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TeamName(String);

impl TeamName {
    /// Borrow the wrapped team name as `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the inner owned name.
    pub fn into_inner(self) -> String {
        self.0
    }

    pub(crate) fn from_validated(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl FromStr for TeamName {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        validate_path_segment(trimmed, "team")?;
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for TeamName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl From<TeamName> for String {
    fn from(value: TeamName) -> Self {
        value.0
    }
}

impl AsRef<str> for TeamName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for TeamName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for TeamName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for TeamName {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// Validated ATM task id carried across command, schema, and hook boundaries.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    /// Borrow the wrapped task id as `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the inner owned task id.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl FromStr for TaskId {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(
                AtmError::validation("task id must not be blank").with_recovery(
                    "Provide a non-empty --task-id value or omit --task-id for non-task messages.",
                ),
            );
        }
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for TaskId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl From<TaskId> for String {
    fn from(value: TaskId) -> Self {
        value.0
    }
}

impl AsRef<str> for TaskId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for TaskId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

const MAX_MODEL_NAME_BYTES: usize = 256;
const MAX_PANE_ID_BYTES: usize = 256;

/// Retained provider/model label carried across roster and config boundaries.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ModelName(String);

impl ModelName {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.len() > MAX_MODEL_NAME_BYTES {
            return Err(AtmError::validation(format!(
                "model must be at most {MAX_MODEL_NAME_BYTES} bytes"
            )));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for ModelName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl From<ModelName> for String {
    fn from(value: ModelName) -> Self {
        value.0
    }
}

impl AsRef<str> for ModelName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for ModelName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for ModelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Claude-compatible pane identifier preserved across roster and config
/// projection boundaries.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PaneId(String);

impl PaneId {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::validation("pane id must not be blank"));
        }
        if trimmed.len() > MAX_PANE_ID_BYTES {
            return Err(AtmError::validation(format!(
                "pane id must be at most {MAX_PANE_ID_BYTES} bytes"
            )));
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn from_cli(value: &str) -> Result<Self, AtmError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::validation("pane id must not be blank"));
        }
        let normalized = if trimmed.starts_with('%') || trimmed.contains(':') {
            trimmed.to_string()
        } else {
            format!("%{trimmed}")
        };
        Self::new(normalized)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for PaneId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl From<PaneId> for String {
    fn from(value: PaneId) -> Self {
        value.0
    }
}

impl AsRef<str> for PaneId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for PaneId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentId, AgentName, ModelName, PaneId, TaskId, TeamName};
    const TEST_AGENT_ID: &str = "test-agent@test-team";

    #[test]
    fn task_id_rejects_blank_deserialization() {
        let error = serde_json::from_str::<TaskId>("\"   \"").expect_err("blank task id");

        assert!(error.to_string().contains("task id must not be blank"));
    }

    #[test]
    fn agent_name_rejects_blank_deserialization() {
        let error = serde_json::from_str::<AgentName>("\"   \"").expect_err("blank agent name");

        assert!(error.to_string().contains("agent"));
    }

    #[test]
    fn team_name_rejects_blank_deserialization() {
        let error = serde_json::from_str::<TeamName>("\"   \"").expect_err("blank team name");

        assert!(error.to_string().contains("team"));
    }

    #[test]
    fn model_name_rejects_overlong_values() {
        let value = "x".repeat(257);
        let error = ModelName::new(value).expect_err("invalid model");

        assert!(error.message.contains("model"));
    }

    #[test]
    fn pane_id_cli_normalizes_plain_numeric_fragment() {
        let pane = PaneId::from_cli("7").expect("pane");

        assert_eq!(pane.as_str(), "%7");
    }

    #[test]
    fn pane_id_deserialize_rejects_blank() {
        let error = serde_json::from_str::<PaneId>("\"   \"").expect_err("blank pane");

        assert!(error.to_string().contains("pane id must not be blank"));
    }

    #[test]
    fn agent_id_round_trips_as_opaque_string() {
        let agent_id: AgentId =
            serde_json::from_str(&format!("\"{TEST_AGENT_ID}\"")).expect("agent id");

        assert_eq!(agent_id.as_str(), TEST_AGENT_ID);
        assert_eq!(
            serde_json::to_string(&agent_id).expect("encode"),
            format!("\"{TEST_AGENT_ID}\"")
        );
    }
}

/// Index of one message within its source mailbox file.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct SourceIndex(usize);

impl SourceIndex {
    /// Return the wrapped zero-based index.
    pub fn get(self) -> usize {
        self.0
    }
}

impl From<usize> for SourceIndex {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl From<SourceIndex> for usize {
    fn from(value: SourceIndex) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnreadReadState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadReadState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoAckState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingAckState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcknowledgedAckState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadState {
    Unread,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AckState {
    NoAckRequired,
    PendingAck,
    Acknowledged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageClass {
    Unread,
    PendingAck,
    Acknowledged,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayBucket {
    Unread,
    PendingAck,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadSelection {
    Actionable,
    Unread,
    PendingAck,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AckActivationMode {
    PromoteDisplayedUnread,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandAction {
    Ack,
    Clear,
    List,
    Read,
    Send,
}
