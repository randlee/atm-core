pub use atm_storage::AckRequirementState;
pub use atm_storage::types::{
    AgentId, AgentIdentity, AgentName, ChatId, HostName, IsoTimestamp, ModelName, PaneId, TaskId,
    TeamName,
};

use std::error::Error;
use std::fmt;

use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Maximum UTF-8 byte length for a session identifier retained in runtime state.
pub const SESSION_ID_MAX_BYTES: usize = 256;

/// Opaque, bounded identifier for one observed runtime session.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(String);

/// Validation error returned when a session identifier exceeds its bounded size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionIdError {
    TooLong {
        max_bytes: usize,
        actual_bytes: usize,
    },
}

impl fmt::Display for SessionIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong {
                max_bytes,
                actual_bytes,
            } => write!(
                formatter,
                "session id is {actual_bytes} bytes; maximum is {max_bytes} bytes"
            ),
        }
    }
}

impl Error for SessionIdError {}

impl SessionId {
    /// Construct a bounded session identifier, treating blank input as absent.
    pub fn new(value: impl AsRef<str>) -> Result<Option<Self>, SessionIdError> {
        let value = value.as_ref();
        if value.trim().is_empty() {
            return Ok(None);
        }

        let actual_bytes = value.len();
        if actual_bytes > SESSION_ID_MAX_BYTES {
            return Err(SessionIdError::TooLong {
                max_bytes: SESSION_ID_MAX_BYTES,
                actual_bytes,
            });
        }

        Ok(Some(Self(value.to_owned())))
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for SessionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value)
            .map_err(de::Error::custom)?
            .ok_or_else(|| de::Error::custom("session id must not be blank"))
    }
}

/// Deserialize an optional session identifier while accepting legacy blank input.
pub fn deserialize_optional_session_id<'de, D>(
    deserializer: D,
) -> Result<Option<SessionId>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(SessionId::new)
        .transpose()
        .map_err(de::Error::custom)
        .map(Option::flatten)
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
pub enum CommandAction {
    Ack,
    Clear,
    List,
    Peek,
    Read,
    Send,
}

#[cfg(test)]
mod tests {
    use super::{SESSION_ID_MAX_BYTES, SessionId, SessionIdError};

    #[test]
    fn session_id_normalizes_blank_input_to_absent() {
        assert_eq!(SessionId::new(" \t\n ").expect("blank is valid"), None);
    }

    #[test]
    fn session_id_accepts_its_byte_limit() {
        let value = "s".repeat(SESSION_ID_MAX_BYTES);
        let session_id = SessionId::new(&value)
            .expect("bounded session id")
            .expect("non-blank session id");

        assert_eq!(session_id.as_ref(), value);
        assert_eq!(session_id.to_string(), value);
    }

    #[test]
    fn session_id_rejects_values_over_its_byte_limit() {
        let value = "s".repeat(SESSION_ID_MAX_BYTES + 1);

        assert_eq!(
            SessionId::new(value),
            Err(SessionIdError::TooLong {
                max_bytes: SESSION_ID_MAX_BYTES,
                actual_bytes: SESSION_ID_MAX_BYTES + 1,
            })
        );
    }
}
