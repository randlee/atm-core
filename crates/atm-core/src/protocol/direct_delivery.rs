use serde::{Deserialize, Serialize};

use crate::schema::InboxMessage;
use crate::types::{AgentName, TeamName};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectDeliveryRequest {
    pub team: TeamName,
    pub agent: AgentName,
    pub messages: Vec<InboxMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectDeliveryOutcome {
    pub delivered_messages: usize,
}
