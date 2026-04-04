use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMember {
    pub name: String,

    #[serde(default)]
    pub agent_id: String,

    #[serde(default)]
    pub agent_type: String,

    #[serde(default)]
    pub model: String,

    #[serde(default)]
    pub joined_at: Option<u64>,

    #[serde(default)]
    pub tmux_pane_id: String,

    #[serde(default)]
    pub cwd: String,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::AgentMember;

    #[test]
    fn parse_name_only_record_defaults_optional_fields() {
        let member: AgentMember = serde_json::from_str(r#"{"name":"arch-ctm"}"#).expect("member");

        assert_eq!(member.name, "arch-ctm");
        assert!(member.agent_id.is_empty());
        assert!(member.agent_type.is_empty());
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
        assert_eq!(member.name, "arch-ctm");
        assert_eq!(member.agent_type, "general-purpose");
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

        assert_eq!(member.name, "arch-ctm");
        assert_eq!(member.agent_type, "plan");
        assert!(member.agent_id.is_empty());
        assert!(member.model.is_empty());
        assert_eq!(member.joined_at, None);
        assert!(member.tmux_pane_id.is_empty());
        assert!(member.cwd.is_empty());
    }
}
