use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::agent_member::AgentMember;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamConfig {
    #[serde(default)]
    pub members: Vec<AgentMember>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
