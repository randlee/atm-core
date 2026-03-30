use serde::{Deserialize, Serialize};

use super::agent_member::AgentMember;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamConfig {
    #[serde(default)]
    pub members: Vec<AgentMember>,
}
