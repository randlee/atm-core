use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::types::{AgentId, AgentName, ModelName, PaneId};
pub use atm_storage::contract::AgentType;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMember {
    pub name: AgentName,

    /// Compound `agent@team` address as supplied by the external Claude Code
    /// agent-team API. Opaque passthrough — format is owned externally and not
    /// validated as an ATM path segment.
    #[serde(default, skip_serializing_if = "AgentId::is_empty")]
    pub agent_id: AgentId,

    /// Agent type as deserialized from Claude Code agent-team config. ATM
    /// reads but does not write config.json — no round-trip concern.
    #[serde(default)]
    pub agent_type: AgentType,

    /// Retained provider/model label copied from `config.json` roster state.
    #[serde(default)]
    pub model: ModelName,

    #[serde(default)]
    pub joined_at: Option<u64>,

    /// Retained tmux pane identifier copied from `config.json` roster state.
    #[serde(default)]
    pub tmux_pane_id: Option<PaneId>,

    /// Durable agent-home directory imported from or projected back into
    /// compatibility config documents.
    #[serde(default, alias = "cwd", rename = "home_dir")]
    pub home_dir: PathBuf,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl AgentMember {
    pub fn with_name(name: AgentName) -> Self {
        Self {
            name,
            agent_id: AgentId::default(),
            agent_type: AgentType::default(),
            model: ModelName::default(),
            joined_at: None,
            tmux_pane_id: None,
            home_dir: PathBuf::new(),
            extra: Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{AgentMember, AgentType};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::AgentName;

    #[test]
    fn parse_name_only_record_defaults_optional_fields() {
        let member: AgentMember =
            serde_json::from_str(&format!(r#"{{"name":"{TEST_SENDER}"}}"#)).expect("member");

        assert_eq!(member.name, AgentName::from_validated(TEST_SENDER));
        assert!(member.agent_id.is_empty());
        assert_eq!(member.agent_type, AgentType::Unknown(String::new()));
        assert!(member.model.is_empty());
        assert_eq!(member.joined_at, None);
        assert_eq!(member.tmux_pane_id, None);
        assert_eq!(member.home_dir, PathBuf::new());
        assert!(member.extra.is_empty());
    }

    #[test]
    fn parse_full_claude_code_record_preserves_values_and_extra() {
        let raw = format!(
            r#"{{
            "agentId":"{TEST_SENDER}@{TEST_TEAM}",
            "name":"{TEST_SENDER}",
            "agentType":"general-purpose",
            "model":"claude-sonnet-4-5",
            "joinedAt":1770765919076,
            "tmuxPaneId":"%1",
            "home_dir":"/workspace",
            "color":"blue"
        }}"#
        );

        let member: AgentMember = serde_json::from_str(&raw).expect("member");
        assert_eq!(
            member.agent_id.as_str(),
            format!("{TEST_SENDER}@{TEST_TEAM}")
        );
        assert_eq!(member.name, AgentName::from_validated(TEST_SENDER));
        assert_eq!(member.agent_type, AgentType::GeneralPurpose);
        assert_eq!(member.model.as_str(), "claude-sonnet-4-5");
        assert_eq!(member.joined_at, Some(1770765919076));
        assert_eq!(member.tmux_pane_id.as_deref(), Some("%1"));
        assert_eq!(member.home_dir, PathBuf::from("/workspace"));
        assert_eq!(member.extra["color"], serde_json::json!("blue"));

        let encoded = serde_json::to_string(&member).expect("encode");
        let decoded: AgentMember = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, member);
    }

    #[test]
    fn parse_legacy_cwd_record_maps_to_home_dir() {
        let member: AgentMember =
            serde_json::from_str(&format!(r#"{{"name":"{TEST_SENDER}","cwd":"/workspace"}}"#))
                .expect("member");

        assert_eq!(member.name, AgentName::from_validated(TEST_SENDER));
        assert_eq!(member.home_dir, PathBuf::from("/workspace"));
    }

    #[test]
    fn parse_name_and_agent_type_record_succeeds() {
        let member: AgentMember =
            serde_json::from_str(&format!(r#"{{"name":"{TEST_SENDER}","agentType":"plan"}}"#))
                .expect("member");

        assert_eq!(member.name, AgentName::from_validated(TEST_SENDER));
        assert_eq!(member.agent_type, AgentType::Plan);
        assert!(member.agent_id.is_empty());
        assert!(member.model.is_empty());
        assert_eq!(member.joined_at, None);
        assert_eq!(member.tmux_pane_id, None);
        assert_eq!(member.home_dir, PathBuf::new());
    }
}
