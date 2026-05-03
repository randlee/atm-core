use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::warn;

use crate::types::AgentName;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AgentType {
    GeneralPurpose,
    Plan,
    Lead,
    Qa,
    Worker,
    Unknown(String),
}

impl Default for AgentType {
    fn default() -> Self {
        Self::Unknown(String::new())
    }
}

impl From<String> for AgentType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "general-purpose" => Self::GeneralPurpose,
            "plan" => Self::Plan,
            "lead" => Self::Lead,
            "qa" => Self::Qa,
            "worker" => Self::Worker,
            _ => {
                warn!(
                    raw_agent_type = %value,
                    "unknown agent_type preserved as opaque compatibility value"
                );
                Self::Unknown(value)
            }
        }
    }
}

impl From<AgentType> for String {
    fn from(value: AgentType) -> Self {
        match value {
            AgentType::GeneralPurpose => "general-purpose".to_string(),
            AgentType::Plan => "plan".to_string(),
            AgentType::Lead => "lead".to_string(),
            AgentType::Qa => "qa".to_string(),
            AgentType::Worker => "worker".to_string(),
            AgentType::Unknown(value) => value,
        }
    }
}

impl Serialize for AgentType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&String::from(self.clone()))
    }
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from(String::deserialize(deserializer)?))
    }
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from(self.clone()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMember {
    pub name: AgentName,

    /// Retained external compatibility field for the full runtime-scoped agent
    /// identifier (for example `arch-ctm@atm-dev`).
    #[serde(default)]
    pub agent_id: String,

    #[serde(default)]
    pub agent_type: AgentType,

    /// Retained provider/model label copied from `config.json` roster state.
    #[serde(default)]
    pub model: String,

    #[serde(default)]
    pub joined_at: Option<u64>,

    /// Retained tmux pane identifier copied from `config.json` roster state.
    #[serde(default)]
    pub tmux_pane_id: String,

    #[serde(default)]
    pub cwd: String,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl AgentMember {
    pub fn with_name(name: AgentName) -> Self {
        Self {
            name,
            agent_id: String::new(),
            agent_type: AgentType::default(),
            model: String::new(),
            joined_at: None,
            tmux_pane_id: String::new(),
            cwd: String::new(),
            extra: Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentMember, AgentType};
    use crate::types::AgentName;

    #[test]
    fn parse_name_only_record_defaults_optional_fields() {
        let member: AgentMember = serde_json::from_str(r#"{"name":"arch-ctm"}"#).expect("member");

        assert_eq!(member.name, AgentName::from_validated("arch-ctm"));
        assert!(member.agent_id.is_empty());
        assert_eq!(member.agent_type, AgentType::Unknown(String::new()));
        assert!(member.model.is_empty());
        assert_eq!(member.joined_at, None);
        assert!(member.tmux_pane_id.is_empty());
        assert!(member.cwd.is_empty());
        assert!(member.extra.is_empty());
    }

    #[test]
    fn parse_full_claude_code_record_preserves_values_and_extra() {
        let raw = r#"{
            "agentId":"arch-ctm@atm-dev",
            "name":"arch-ctm",
            "agentType":"general-purpose",
            "model":"claude-sonnet-4-5",
            "joinedAt":1770765919076,
            "tmuxPaneId":"%1",
            "cwd":"/workspace",
            "color":"blue"
        }"#;

        let member: AgentMember = serde_json::from_str(raw).expect("member");
        assert_eq!(member.agent_id, "arch-ctm@atm-dev");
        assert_eq!(member.name, AgentName::from_validated("arch-ctm"));
        assert_eq!(member.agent_type, AgentType::GeneralPurpose);
        assert_eq!(member.model, "claude-sonnet-4-5");
        assert_eq!(member.joined_at, Some(1770765919076));
        assert_eq!(member.tmux_pane_id, "%1");
        assert_eq!(member.cwd, "/workspace");
        assert_eq!(member.extra["color"], serde_json::json!("blue"));

        let encoded = serde_json::to_string(&member).expect("encode");
        let decoded: AgentMember = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, member);
    }

    #[test]
    fn parse_name_and_agent_type_record_succeeds() {
        let member: AgentMember =
            serde_json::from_str(r#"{"name":"arch-ctm","agentType":"plan"}"#).expect("member");

        assert_eq!(member.name, AgentName::from_validated("arch-ctm"));
        assert_eq!(member.agent_type, AgentType::Plan);
        assert!(member.agent_id.is_empty());
        assert!(member.model.is_empty());
        assert_eq!(member.joined_at, None);
        assert!(member.tmux_pane_id.is_empty());
        assert!(member.cwd.is_empty());
    }
}
