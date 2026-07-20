use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::AtmError;
use crate::validation::{validate_agent_at_team, validate_path_segment};

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

impl FromStr for IsoTimestamp {
    type Err = chrono::ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse::<DateTime<Utc>>().map(Self::from_datetime)
    }
}

impl From<DateTime<Utc>> for IsoTimestamp {
    fn from(datetime: DateTime<Utc>) -> Self {
        Self(datetime)
    }
}

impl fmt::Display for IsoTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.to_rfc3339())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentName(String);

impl AgentName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    #[doc(hidden)]
    pub fn from_validated(value: impl Into<String>) -> Self {
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

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        let trimmed = value.trim();
        validate_agent_at_team(trimmed, "agent id")?;
        Ok(Self(trimmed.to_string()))
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

impl FromStr for AgentId {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for AgentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
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

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TeamName(String);

impl TeamName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    #[doc(hidden)]
    pub fn from_validated(value: impl Into<String>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::{AgentId, AgentName, IsoTimestamp};

    #[test]
    fn agent_id_new_matches_agent_name_validation_for_valid_value() {
        let agent_id = AgentId::new("worker-1").expect("agent id");
        let agent_name: AgentName = "worker-1".parse().expect("agent name");

        assert_eq!(agent_id.as_str(), agent_name.as_str());
    }

    #[test]
    fn agent_id_new_rejects_invalid_path_segments() {
        for invalid in [
            "",
            ".hidden",
            "two..dots",
            "bad/name",
            "bad name",
            "role@bad.team",
        ] {
            assert!(
                AgentId::new(invalid).is_err(),
                "expected `{invalid}` to fail"
            );
        }
    }

    #[test]
    fn agent_id_deserialize_applies_validation() {
        let parsed: AgentId = serde_json::from_str("\"worker-2\"").expect("deserialize");
        assert_eq!(parsed.as_str(), "worker-2");
        let compound: AgentId =
            serde_json::from_str("\"worker-2@test-team\"").expect("compound deserialize");
        assert_eq!(compound.as_str(), "worker-2@test-team");

        let error = serde_json::from_str::<AgentId>("\"bad/name\"").expect_err("invalid id");
        assert!(
            error
                .to_string()
                .contains("must use only ASCII letters, digits, '-' or '_'")
        );
    }

    #[test]
    fn iso_timestamp_from_str_round_trips_rfc3339_input() {
        let timestamp: IsoTimestamp = "2026-07-11T01:20:17Z".parse().expect("timestamp");

        assert_eq!(timestamp.to_string(), "2026-07-11T01:20:17+00:00");
    }
}

impl PartialEq<&str> for TeamName {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl FromStr for TaskId {
    type Err = AtmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::validation("task id must not be blank"));
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
