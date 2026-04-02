use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AgentConfig {
    pub post_send: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AtmConfig {
    pub identity: Option<String>,
    pub default_team: Option<String>,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
}
