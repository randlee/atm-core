use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::types::{AgentId, AgentName, ModelName, PaneId};
pub use atm_storage::contract::AgentType;

pub const HOME_DIR_METADATA_KEY: &str = "home_dir";
/// Optional host workspace root used by graft receivers. When present it is
/// the canonical root for the receiver endpoint; `home_dir` remains the
/// compatibility fallback for roster members that predate this field.
pub const WORKSPACE_ROOT_METADATA_KEY: &str = "workspace_root";
#[deprecated(note = "Phase AD obsolete: derived compatibility field only")]
pub const LEGACY_CWD_METADATA_KEY: &str = "cwd";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HomeDirPath(PathBuf);

impl HomeDirPath {
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.as_os_str().is_empty()
    }
}

impl AsRef<Path> for HomeDirPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl From<PathBuf> for HomeDirPath {
    fn from(value: PathBuf) -> Self {
        Self(value)
    }
}

impl From<HomeDirPath> for PathBuf {
    fn from(value: HomeDirPath) -> Self {
        value.0
    }
}

pub fn canonical_home_dir(metadata_json: &Map<String, Value>) -> Option<HomeDirPath> {
    metadata_home_dir(metadata_json, HOME_DIR_METADATA_KEY)
}

pub fn canonical_graft_root(metadata_json: &Map<String, Value>) -> Option<HomeDirPath> {
    metadata_home_dir(metadata_json, WORKSPACE_ROOT_METADATA_KEY)
        .or_else(|| canonical_home_dir(metadata_json))
}

#[allow(
    deprecated,
    reason = "Phase AD obsolete: bounded fallback remains only to read pre-AD compatibility metadata."
)]
pub fn compatible_home_dir(metadata_json: &Map<String, Value>) -> Option<HomeDirPath> {
    canonical_home_dir(metadata_json)
        .or_else(|| metadata_home_dir(metadata_json, LEGACY_CWD_METADATA_KEY))
}

fn metadata_home_dir(metadata_json: &Map<String, Value>, key: &str) -> Option<HomeDirPath> {
    metadata_json
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| PathBuf::from(value).into())
}

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
    pub home_dir: HomeDirPath,

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
            home_dir: HomeDirPath::default(),
            extra: Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::{Map, Value};

    use super::{
        AgentMember, AgentType, HOME_DIR_METADATA_KEY, HomeDirPath, WORKSPACE_ROOT_METADATA_KEY,
        canonical_graft_root, canonical_home_dir, compatible_home_dir,
    };
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
        assert_eq!(member.home_dir, HomeDirPath::default());
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
        assert_eq!(
            member.home_dir.as_path(),
            PathBuf::from("/workspace").as_path()
        );
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
        assert_eq!(
            member.home_dir.as_path(),
            PathBuf::from("/workspace").as_path()
        );
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
        assert_eq!(member.home_dir, HomeDirPath::default());
    }

    #[test]
    fn canonical_home_dir_reads_typed_metadata() {
        let metadata =
            Map::from_iter([(HOME_DIR_METADATA_KEY.to_string(), Value::from("/repo/home"))]);

        assert_eq!(
            canonical_home_dir(&metadata)
                .as_ref()
                .map(HomeDirPath::as_path),
            Some(Path::new("/repo/home"))
        );
    }

    #[test]
    fn canonical_graft_root_prefers_workspace_root_over_profile_home() {
        let metadata = Map::from_iter([
            (
                HOME_DIR_METADATA_KEY.to_string(),
                Value::from("/profile/home"),
            ),
            (
                WORKSPACE_ROOT_METADATA_KEY.to_string(),
                Value::from("/workspace/root"),
            ),
        ]);

        assert_eq!(
            canonical_graft_root(&metadata)
                .as_ref()
                .map(HomeDirPath::as_path),
            Some(Path::new("/workspace/root"))
        );
    }

    #[test]
    #[allow(
        deprecated,
        reason = "Phase AD obsolete: bounded fallback remains only for pre-AD compatibility metadata."
    )]
    fn compatible_home_dir_falls_back_to_legacy_cwd() {
        let metadata = Map::from_iter([(
            super::LEGACY_CWD_METADATA_KEY.to_string(),
            Value::from("/repo/cwd"),
        )]);

        assert_eq!(
            compatible_home_dir(&metadata)
                .as_ref()
                .map(HomeDirPath::as_path),
            Some(Path::new("/repo/cwd"))
        );
    }
}
