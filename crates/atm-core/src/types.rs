pub use atm_storage::AckRequirementState;
pub(crate) use atm_storage::types::deserialize_optional_session_id;
pub use atm_storage::types::{
    AgentId, AgentIdentity, AgentName, ChatId, HostName, IsoTimestamp, ModelName, PaneId,
    SESSION_ID_MAX_BYTES, SessionId, SessionIdError, TaskId, TeamName,
};

use serde::{Deserialize, Serialize};

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
        assert_eq!(
            SessionId::parse_optional(" \t\n ").expect("blank is valid"),
            None
        );
    }

    #[test]
    fn session_id_accepts_its_byte_limit() {
        let value = "s".repeat(SESSION_ID_MAX_BYTES);
        let session_id = SessionId::new(&value).expect("bounded session id");

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
