use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IsoTimestamp(pub DateTime<Utc>);

impl IsoTimestamp {
    pub fn now() -> Self {
        Self(Utc::now())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageClass {
    Unread,
    PendingAck,
    Acknowledged,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayBucket {
    Unread,
    PendingAck,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadSelection {
    Actionable,
    UnreadOnly,
    PendingAckOnly,
    ActionableWithHistory,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AckActivationMode {
    PromoteDisplayedUnread,
    ReadOnly,
}
