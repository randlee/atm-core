use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::warn;

use crate::types::AgentName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentType {
    GeneralPurpose,
    Plan,
    Lead,
    Qa,
    Worker,
    Unknown(String),
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

    /// Compound `agent@team` address as supplied by the external Claude Code
    /// agent-team API. Opaque passthrough — format is owned externally and not
    /// validated as an ATM path segment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    /// Agent type as deserialized from Claude Code agent-team config. ATM
    /// reads but does not write config.json — no round-trip concern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<AgentType>,

    /// Retained provider/model label copied from `config.json` roster state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(default)]
    pub joined_at: Option<u64>,

    /// Retained tmux pane identifier copied from `config.json` roster state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_pane_id: Option<String>,

    /// Retained working directory path for the agent process, copied from `config.json` roster state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl AgentMember {
    pub fn with_name(name: AgentName) -> Self {
        Self {
            name,
            agent_id: None,
            agent_type: None,
            model: None,
            joined_at: None,
            tmux_pane_id: None,
            cwd: None,
            extra: Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentMember, AgentType};
    use crate::types::AgentName;

    #[test]
    fn with_name_constructs_explicit_member_without_hidden_identity_defaults() {
        let member = AgentMember::with_name(AgentName::from_validated("arch-ctm"));

        assert_eq!(member.name, AgentName::from_validated("arch-ctm"));
        assert_eq!(member.agent_id, None);
        assert_eq!(member.agent_type, None);
        assert_eq!(member.model, None);
        assert_eq!(member.joined_at, None);
        assert_eq!(member.tmux_pane_id, None);
        assert_eq!(member.cwd, None);
        assert!(member.extra.is_empty());
    }

    #[test]
    fn parse_name_only_record_defaults_optional_fields() {
        let member: AgentMember = serde_json::from_str(r#"{"name":"arch-ctm"}"#).expect("member");

        assert_eq!(member.name, AgentName::from_validated("arch-ctm"));
        assert_eq!(member.agent_id, None);
        assert_eq!(member.agent_type, None);
        assert_eq!(member.model, None);
        assert_eq!(member.joined_at, None);
        assert_eq!(member.tmux_pane_id, None);
        assert_eq!(member.cwd, None);
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
        assert_eq!(member.agent_id.as_deref(), Some("arch-ctm@atm-dev"));
        assert_eq!(member.name, AgentName::from_validated("arch-ctm"));
        assert_eq!(member.agent_type, Some(AgentType::GeneralPurpose));
        assert_eq!(member.model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(member.joined_at, Some(1770765919076));
        assert_eq!(member.tmux_pane_id.as_deref(), Some("%1"));
        assert_eq!(member.cwd.as_deref(), Some("/workspace"));
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
        assert_eq!(member.agent_type, Some(AgentType::Plan));
        assert_eq!(member.agent_id, None);
        assert_eq!(member.model, None);
        assert_eq!(member.joined_at, None);
        assert_eq!(member.tmux_pane_id, None);
        assert_eq!(member.cwd, None);
    }
}
