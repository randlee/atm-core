pub mod aliases;
pub mod bridge;
pub mod discovery;
pub mod types;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tracing::warn;

pub use types::AtmConfig;

use crate::error::{AtmError, AtmErrorKind};
use crate::schema::{AgentMember, TeamConfig};

pub fn load_config(start_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
    let Some(path) = find_config_path(start_dir) else {
        return Ok(None);
    };

    let contents = fs::read_to_string(&path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!("failed to read config at {}: {error}", path.display()),
        )
        .with_source(error)
    })?;
    let parsed = toml::from_str(&contents).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!("failed to parse config at {}: {error}", path.display()),
        )
        .with_source(error)
    })?;
    Ok(Some(parsed))
}

pub fn load_team_config(team_dir: &Path) -> Result<TeamConfig, AtmError> {
    let config_path = team_dir.join("config.json");
    let raw = fs::read_to_string(&config_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AtmError::missing_document(format!(
                "team config is missing at {}",
                config_path.display()
            ))
            .with_recovery(
                "Restore config.json for the team or use only the documented send fallback.",
            )
            .with_source(error)
        } else {
            AtmError::new(
                AtmErrorKind::Config,
                format!(
                    "failed to read team config at {}: {error}",
                    config_path.display()
                ),
            )
            .with_recovery("Create config.json or restore it from a known-good copy.")
            .with_source(error)
        }
    })?;

    parse_team_config(&config_path, &raw)
}

pub fn resolve_identity(config: Option<&AtmConfig>) -> Option<String> {
    env::var("ATM_IDENTITY")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| config.and_then(|cfg| cfg.identity.clone()))
}

pub fn resolve_team(team_override: Option<&str>, config: Option<&AtmConfig>) -> Option<String> {
    team_override
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| env::var("ATM_TEAM").ok().filter(|value| !value.is_empty()))
        .or_else(|| config.and_then(|cfg| cfg.default_team.clone()))
}

fn find_config_path(start_dir: &Path) -> Option<PathBuf> {
    let mut current = Some(start_dir);

    while let Some(dir) = current {
        let candidate = dir.join(".atm.toml");
        if candidate.is_file() {
            return Some(candidate);
        }

        current = dir.parent();
    }

    None
}

fn parse_team_config(config_path: &Path, raw: &str) -> Result<TeamConfig, AtmError> {
    let root: Value = serde_json::from_str(raw).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "failed to parse team config at {}: {error}",
                config_path.display()
            ),
        )
        .with_recovery("Repair the JSON syntax in config.json or restore a valid file.")
        .with_source(error)
    })?;

    let object = root.as_object().ok_or_else(|| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "failed to parse team config at {}: root value must be a JSON object",
                config_path.display()
            ),
        )
        .with_recovery("Repair config.json so the root value is an object with a 'members' array.")
    })?;

    let members = match object.get("members") {
        None => Vec::new(),
        Some(Value::Array(entries)) => entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| parse_team_member(config_path, index, entry))
            .collect(),
        Some(_) => {
            return Err(AtmError::new(
                AtmErrorKind::Config,
                format!(
                    "failed to parse team config at {}: field 'members' must be a JSON array",
                    config_path.display()
                ),
            )
            .with_recovery(
                "Repair config.json so 'members' is an array of agent records or agent names.",
            ));
        }
    };

    Ok(TeamConfig { members })
}

fn parse_team_member(config_path: &Path, index: usize, entry: &Value) -> Option<AgentMember> {
    match entry {
        Value::String(name) => Some(AgentMember { name: name.clone() }),
        _ => match serde_json::from_value::<AgentMember>(entry.clone()) {
            Ok(member) => Some(member),
            Err(error) => {
                let member_label = entry
                    .as_object()
                    .and_then(|object| object.get("name"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("#{index}"));
                warn!(
                    path = %config_path.display(),
                    member_index = index,
                    member = %member_label,
                    %error,
                    "skipping invalid team member record"
                );
                None
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{load_config, parse_team_config, resolve_identity, resolve_team, AtmConfig};

    #[test]
    fn load_config_walks_upward_for_dot_atm_toml() {
        let root = unique_temp_dir("config-discovery");
        let nested = root.join("workspace").join("nested");
        fs::create_dir_all(&nested).expect("nested dir");
        fs::write(
            root.join(".atm.toml"),
            "identity = \"arch-ctm\"\ndefault_team = \"atm-dev\"\n",
        )
        .expect("config");

        let config = load_config(&nested).expect("config").expect("present");
        assert_eq!(config.identity.as_deref(), Some("arch-ctm"));
        assert_eq!(config.default_team.as_deref(), Some("atm-dev"));
    }

    #[test]
    fn parse_team_config_accepts_object_members() {
        let config = parse_team_config(
            Path::new("/tmp/config.json"),
            r#"{"members":[{"name":"arch-ctm"},{"name":"team-lead"}]}"#,
        )
        .expect("team config");

        assert_eq!(config.members.len(), 2);
        assert_eq!(config.members[0].name, "arch-ctm");
        assert_eq!(config.members[1].name, "team-lead");
    }

    #[test]
    fn parse_team_config_accepts_string_member_compatibility() {
        let config = parse_team_config(
            Path::new("/tmp/config.json"),
            r#"{"members":["arch-ctm",{"name":"team-lead"}]}"#,
        )
        .expect("team config");

        assert_eq!(config.members.len(), 2);
        assert_eq!(config.members[0].name, "arch-ctm");
        assert_eq!(config.members[1].name, "team-lead");
    }

    #[test]
    fn parse_team_config_skips_invalid_member_records() {
        let config = parse_team_config(
            Path::new("/tmp/config.json"),
            r#"{"members":[{"name":"arch-ctm"},{"broken":true},17,{"name":"team-lead"}]}"#,
        )
        .expect("team config");

        assert_eq!(config.members.len(), 2);
        assert_eq!(config.members[0].name, "arch-ctm");
        assert_eq!(config.members[1].name, "team-lead");
    }

    #[test]
    fn parse_team_config_defaults_missing_members_to_empty() {
        let config =
            parse_team_config(Path::new("/tmp/config.json"), r#"{}"#).expect("team config");

        assert!(config.members.is_empty());
    }

    #[test]
    fn parse_team_config_reports_json_syntax_errors_with_detail() {
        let error = parse_team_config(
            Path::new("/tmp/config.json"),
            r#"{"members":[{"name":"arch-ctm"}"#,
        )
        .expect_err("syntax error");

        assert!(error.is_config());
        assert!(error.message.contains("/tmp/config.json"));
        assert!(error.message.contains("EOF while parsing"));
        assert!(error.recovery.as_deref().is_some());
    }

    #[test]
    fn parse_team_config_rejects_non_object_root() {
        let error = parse_team_config(Path::new("/tmp/config.json"), r#"["arch-ctm"]"#)
            .expect_err("root shape error");

        assert!(error.is_config());
        assert!(error.message.contains("root value must be a JSON object"));
        assert!(error.recovery.as_deref().is_some());
    }

    #[test]
    fn parse_team_config_rejects_non_array_members() {
        let error = parse_team_config(
            Path::new("/tmp/config.json"),
            r#"{"members":{"name":"arch-ctm"}}"#,
        )
        .expect_err("members shape error");

        assert!(error.is_config());
        assert!(error
            .message
            .contains("field 'members' must be a JSON array"));
        assert!(error.recovery.as_deref().is_some());
    }

    #[test]
    fn load_team_config_reports_missing_document_distinctly() {
        let root = unique_temp_dir("missing-team-config");
        let team_dir = root.join("team");
        fs::create_dir_all(&team_dir).expect("team dir");

        let error = super::load_team_config(&team_dir).expect_err("missing config");

        assert!(error.is_missing_document());
        assert!(error.message.contains("team config is missing"));
        assert!(error.recovery.as_deref().is_some());
    }

    #[test]
    #[serial_test::serial]
    fn identity_prefers_environment_over_config() {
        let original_identity = env::var_os("ATM_IDENTITY");
        env::set_var("ATM_IDENTITY", "env-identity");

        let config = AtmConfig {
            identity: Some("config-identity".into()),
            default_team: None,
        };

        assert_eq!(
            resolve_identity(Some(&config)).as_deref(),
            Some("env-identity")
        );
        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    #[serial_test::serial]
    fn team_resolution_prefers_flag_then_env_then_config() {
        let original_team = env::var_os("ATM_TEAM");
        env::set_var("ATM_TEAM", "env-team");

        let config = AtmConfig {
            identity: None,
            default_team: Some("config-team".into()),
        };

        assert_eq!(
            resolve_team(Some("flag-team"), Some(&config)).as_deref(),
            Some("flag-team")
        );
        assert_eq!(
            resolve_team(None, Some(&config)).as_deref(),
            Some("env-team")
        );

        env::remove_var("ATM_TEAM");
        assert_eq!(
            resolve_team(None, Some(&config)).as_deref(),
            Some("config-team")
        );

        restore("ATM_TEAM", original_team);
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn restore(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
        }
    }
}
