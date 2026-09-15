use serde::{Deserialize, Serialize};

use crate::types::AgentName;

use super::MailboxBucket;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMailboxSelection {
    Actionable,
    Unread,
    PendingAck,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchReadState {
    Unread,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchAckState {
    Pending,
    Acknowledged,
    NotRequired,
}

/// Aggregate grouping supported by the storage-owned message counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchCountGroupBy {
    Bucket,
    FromAgent,
    Tag,
}

/// One SQL aggregate result. A bucket is absent for an ungrouped count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCount {
    pub key: Option<SearchCountKey>,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchCountKey {
    Bucket(MailboxBucket),
    FromAgent(AgentName),
    Tag(String),
}
