//! ATM config discovery, loading, normalization, and team-config parsing.

pub mod aliases;
pub mod bridge;
pub mod discovery;
pub mod types;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use toml::Value as TomlValue;
use tracing::warn;

pub use types::AtmConfig;

use crate::error::{AtmError, AtmErrorCode, AtmErrorKind};
use crate::schema::{AgentMember, TeamConfig};

/// Load `.atm.toml` by walking upward from `start_dir`.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::ConfigParseFailed`] when the config file
/// cannot be read or parsed as TOML.
pub fn load_config(start_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
    let Some(path) = find_config_path(start_dir) else {
        return Ok(None);
    };

    let contents = fs::read_to_string(&path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!("failed to read config at {}: {error}", path.display()),
        )
        .with_recovery("Check .atm.toml permissions and syntax, or run the command from a directory inside the intended ATM workspace.")
        .with_source(error)
    })?;
    let raw_toml = toml::from_str::<TomlValue>(&contents).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!("failed to parse config at {}: {error}", path.display()),
        )
        .with_recovery(
            "Repair the .atm.toml syntax or remove malformed ATM config keys before retrying.",
        )
        .with_source(error)
    })?;
    reject_retired_post_send_hook_members(&path, &raw_toml)?;
    let parsed = raw_toml.try_into::<RawConfigFile>().map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!("failed to parse config at {}: {error}", path.display()),
        )
        .with_recovery(
            "Repair the .atm.toml syntax or remove malformed ATM config keys before retrying.",
        )
        .with_source(error)
    })?;
    let obsolete_identity_present = parsed.atm.identity.is_some() || parsed.identity.is_some();
    let config_root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    Ok(Some(AtmConfig {
        identity: parsed.atm.identity.or(parsed.identity),
        default_team: parsed.atm.default_team.or(parsed.default_team),
        team_members: normalize_string_list(parsed.atm.team_members),
        aliases: normalize_aliases(parsed.atm.aliases),
        post_send_hook: normalize_optional_command(parsed.atm.post_send_hook),
        post_send_hook_senders: normalize_string_list(parsed.atm.post_send_hook_senders),
        post_send_hook_recipients: normalize_string_list(parsed.atm.post_send_hook_recipients),
        config_root,
        obsolete_identity_present,
    }))
}

/// Load and validate `config.json` for a team directory.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::ConfigTeamMissing`] when the team config
/// document does not exist, or
/// [`crate::error_codes::AtmErrorCode::ConfigTeamParseFailed`] when the JSON
/// document is malformed or violates the required team-config shape.
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

/// Resolves the sender identity for outgoing messages.
///
/// The `_config` parameter is reserved for a future config-provided identity
/// fallback and is currently unused. Identity is resolved exclusively via the
/// `ATM_IDENTITY` environment variable.
pub fn resolve_identity(_config: Option<&AtmConfig>) -> Option<String> {
    env::var("ATM_IDENTITY")
        .ok()
        .filter(|value| !value.is_empty())
}

/// Resolve the active team from explicit override, environment, or config.
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

#[derive(Debug, Default, Deserialize)]
struct RawConfigFile {
    #[serde(default)]
    atm: RawAtmSection,
    #[serde(default)]
    identity: Option<String>,
    #[serde(default)]
    default_team: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAtmSection {
    #[serde(default)]
    identity: Option<String>,
    #[serde(default)]
    default_team: Option<String>,
    #[serde(default)]
    team_members: Vec<String>,
    #[serde(default)]
    aliases: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    post_send_hook: Option<Vec<String>>,
    #[serde(default)]
    post_send_hook_senders: Vec<String>,
    #[serde(default)]
    post_send_hook_recipients: Vec<String>,
}

fn reject_retired_post_send_hook_members(
    path: &Path,
    raw_toml: &TomlValue,
) -> Result<(), AtmError> {
    let retired_present = raw_toml
        .get("atm")
        .and_then(TomlValue::as_table)
        .is_some_and(|atm| atm.contains_key("post_send_hook_members"));
    if retired_present {
        return Err(AtmError::new_with_code(
            AtmErrorCode::ConfigRetiredHookMembersKey,
            AtmErrorKind::Config,
            format!(
                "error: '{}' field 'post_send_hook_members' is no longer supported.",
                path.display()
            ),
        )
        .with_recovery(
            "Use 'post_send_hook_senders' (match on sender identity) and/or 'post_send_hook_recipients' (match on recipient name) under [atm]. Use '*' to match all senders or all recipients.",
        ));
    }
    Ok(())
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_aliases(
    aliases: std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    aliases
        .into_iter()
        .map(|(alias, canonical)| (alias.trim().to_string(), canonical.trim().to_string()))
        .filter(|(alias, canonical)| !alias.is_empty() && !canonical.is_empty())
        .collect()
}

fn normalize_optional_command(command: Option<Vec<String>>) -> Option<Vec<String>> {
    command.and_then(|values| {
        let normalized = normalize_string_list(values);
        (!normalized.is_empty()).then_some(normalized)
    })
}

fn parse_team_config(config_path: &Path, raw: &str) -> Result<TeamConfig, AtmError> {
    let root: Value = serde_json::from_str(raw).map_err(|error| {
        AtmError::new_with_code(
            AtmErrorCode::ConfigTeamParseFailed,
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
        AtmError::new_with_code(
            AtmErrorCode::ConfigTeamParseFailed,
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
            return Err(AtmError::new_with_code(
                AtmErrorCode::ConfigTeamParseFailed,
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

    let mut extra = object.clone();
    extra.remove("members");

    Ok(TeamConfig { members, extra })
}

fn parse_team_member(config_path: &Path, index: usize, entry: &Value) -> Option<AgentMember> {
    match entry {
        Value::String(name) => Some(AgentMember {
            name: name.clone(),
            ..Default::default()
        }),
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
                    code = %AtmErrorCode::WarningInvalidTeamMemberSkipped,
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
    use crate::error_codes::AtmErrorCode;
    use serde_json::Value;
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    use super::{AtmConfig, load_config, parse_team_config, resolve_identity, resolve_team};

    #[test]
    fn load_config_walks_upward_for_dot_atm_toml() {
        let root = unique_temp_dir("config-discovery");
        let nested = root.join("workspace").join("nested");
        fs::create_dir_all(&nested).expect("nested dir");
        fs::write(
            root.join(".atm.toml"),
            "[atm]\nidentity = \"arch-ctm\"\ndefault_team = \"atm-dev\"\n",
        )
        .expect("config");

        let config = load_config(&nested).expect("config").expect("present");
        assert_eq!(config.identity.as_deref(), Some("arch-ctm"));
        assert_eq!(config.default_team.as_deref(), Some("atm-dev"));
        assert_eq!(config.config_root, root);
        assert!(config.obsolete_identity_present);
    }

    #[test]
    fn load_config_accepts_legacy_top_level_keys_for_compatibility() {
        let root = unique_temp_dir("legacy-config");
        fs::write(
            root.join(".atm.toml"),
            "identity = \"arch-ctm\"\ndefault_team = \"atm-dev\"\n",
        )
        .expect("config");

        let config = load_config(&root).expect("config").expect("present");
        assert_eq!(config.identity.as_deref(), Some("arch-ctm"));
        assert_eq!(config.default_team.as_deref(), Some("atm-dev"));
        assert_eq!(config.config_root, root);
        assert!(config.obsolete_identity_present);
    }

    #[test]
    fn load_config_reads_team_members_aliases_and_post_send_hook() {
        let root = unique_temp_dir("atm-config-surface");
        fs::write(
            root.join(".atm.toml"),
            r#"[atm]
default_team = "atm-dev"
team_members = ["team-lead", "arch-ctm", " ", "qa"]
post_send_hook = ["bin/hook", "notify"]
post_send_hook_senders = ["arch-ctm", "", "team-lead"]
post_send_hook_recipients = ["quality-mgr", "", "*"]

[atm.aliases]
tl = "team-lead"
qa = "quality-mgr"
blank = ""
"#,
        )
        .expect("config");

        let config = load_config(&root).expect("config").expect("present");
        assert_eq!(config.team_members, vec!["team-lead", "arch-ctm", "qa"]);
        assert_eq!(
            config.post_send_hook.as_deref(),
            Some(&["bin/hook".to_string(), "notify".to_string()][..])
        );
        assert_eq!(
            config.post_send_hook_senders,
            vec!["arch-ctm".to_string(), "team-lead".to_string()]
        );
        assert_eq!(
            config.post_send_hook_recipients,
            vec!["quality-mgr".to_string(), "*".to_string()]
        );
        assert_eq!(
            config.aliases.get("tl").map(String::as_str),
            Some("team-lead")
        );
        assert_eq!(
            config.aliases.get("qa").map(String::as_str),
            Some("quality-mgr")
        );
        assert!(!config.aliases.contains_key("blank"));
    }

    #[test]
    fn load_config_ignores_core_section_hook_keys() {
        let root = unique_temp_dir("core-config-hook-keys");
        fs::write(
            root.join(".atm.toml"),
            r#"[core]
default_team = "atm-dev"
identity = "team-lead"
post_send_hook = ["bin/hook", "notify"]
post_send_hook_senders = ["team-lead"]
post_send_hook_recipients = ["arch-ctm"]
"#,
        )
        .expect("config");

        let config = load_config(&root).expect("config").expect("present");
        assert_eq!(config.default_team, None);
        assert_eq!(config.identity, None);
        assert_eq!(config.post_send_hook, None);
        assert!(config.post_send_hook_senders.is_empty());
        assert!(config.post_send_hook_recipients.is_empty());
        assert!(!config.obsolete_identity_present);
    }

    #[test]
    fn load_config_rejects_retired_post_send_hook_members_key() {
        let root = unique_temp_dir("retired-hook-members");
        fs::write(
            root.join(".atm.toml"),
            r#"[atm]
post_send_hook = ["bin/hook"]
post_send_hook_members = ["team-lead"]
"#,
        )
        .expect("config");

        let error = load_config(&root).expect_err("retired key should fail");

        assert!(error.is_config());
        assert_eq!(error.code, AtmErrorCode::ConfigRetiredHookMembersKey);
        assert!(
            error
                .message
                .contains(&root.join(".atm.toml").display().to_string())
        );
        assert!(error.message.contains("post_send_hook_members"));
        assert_eq!(
            error.recovery.as_deref(),
            Some(
                "Use 'post_send_hook_senders' (match on sender identity) and/or 'post_send_hook_recipients' (match on recipient name) under [atm]. Use '*' to match all senders or all recipients."
            )
        );
    }

    #[test]
    fn parse_team_config_accepts_object_members() {
        let config_path = temp_config_path();
        let config = parse_team_config(
            &config_path,
            r#"{"members":[{"name":"arch-ctm"},{"name":"team-lead"}]}"#,
        )
        .expect("team config");

        assert_eq!(config.members.len(), 2);
        assert_eq!(config.members[0].name, "arch-ctm");
        assert_eq!(config.members[1].name, "team-lead");
        assert!(config.extra.is_empty());
    }

    #[test]
    fn parse_team_config_accepts_string_member_compatibility() {
        let config_path = temp_config_path();
        let config = parse_team_config(
            &config_path,
            r#"{"members":["arch-ctm",{"name":"team-lead"}]}"#,
        )
        .expect("team config");

        assert_eq!(config.members.len(), 2);
        assert_eq!(config.members[0].name, "arch-ctm");
        assert_eq!(config.members[1].name, "team-lead");
        assert!(config.extra.is_empty());
    }

    #[test]
    fn parse_team_config_skips_invalid_member_records() {
        let config_path = temp_config_path();
        let config = parse_team_config(
            &config_path,
            r#"{"members":[{"name":"arch-ctm"},{"broken":true},17,{"name":"team-lead"}]}"#,
        )
        .expect("team config");

        assert_eq!(config.members.len(), 2);
        assert_eq!(config.members[0].name, "arch-ctm");
        assert_eq!(config.members[1].name, "team-lead");
        assert!(config.extra.is_empty());
    }

    #[test]
    fn parse_team_config_defaults_missing_members_to_empty() {
        let config_path = temp_config_path();
        let config = parse_team_config(&config_path, r#"{}"#).expect("team config");

        assert!(config.members.is_empty());
        assert!(config.extra.is_empty());
    }

    #[test]
    fn parse_team_config_preserves_root_extra_fields() {
        let config_path = temp_config_path();
        let config = parse_team_config(
            &config_path,
            r#"{"leadSessionId":"lead-123","members":[{"name":"team-lead"}]}"#,
        )
        .expect("team config");

        assert_eq!(config.members.len(), 1);
        assert_eq!(
            config.extra["leadSessionId"],
            Value::String("lead-123".to_string())
        );
    }

    #[test]
    fn parse_team_config_reports_json_syntax_errors_with_detail() {
        let config_path = temp_config_path();
        let error = parse_team_config(&config_path, r#"{"members":[{"name":"arch-ctm"}"#)
            .expect_err("syntax error");

        assert!(error.is_config());
        assert_eq!(error.code, AtmErrorCode::ConfigTeamParseFailed);
        assert!(error.message.contains("config.json"));
        assert!(error.message.contains("EOF while parsing"));
        assert!(error.recovery.as_deref().is_some());
    }

    #[test]
    fn parse_team_config_rejects_non_object_root() {
        let config_path = temp_config_path();
        let error =
            parse_team_config(&config_path, r#"["arch-ctm"]"#).expect_err("root shape error");

        assert!(error.is_config());
        assert_eq!(error.code, AtmErrorCode::ConfigTeamParseFailed);
        assert!(error.message.contains("root value must be a JSON object"));
        assert!(error.recovery.as_deref().is_some());
    }

    #[test]
    fn parse_team_config_rejects_non_array_members() {
        let config_path = temp_config_path();
        let error = parse_team_config(&config_path, r#"{"members":{"name":"arch-ctm"}}"#)
            .expect_err("members shape error");

        assert!(error.is_config());
        assert_eq!(error.code, AtmErrorCode::ConfigTeamParseFailed);
        assert!(
            error
                .message
                .contains("field 'members' must be a JSON array")
        );
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
        set_env_var("ATM_IDENTITY", "env-identity");

        let config = AtmConfig {
            identity: Some("config-identity".into()),
            obsolete_identity_present: true,
            ..Default::default()
        };

        assert_eq!(
            resolve_identity(Some(&config)).as_deref(),
            Some("env-identity")
        );
        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    #[serial_test::serial]
    fn identity_ignores_obsolete_config_field_when_env_missing() {
        let original_identity = env::var_os("ATM_IDENTITY");
        remove_env_var("ATM_IDENTITY");

        let config = AtmConfig {
            identity: Some("config-identity".into()),
            obsolete_identity_present: true,
            ..Default::default()
        };

        assert_eq!(resolve_identity(Some(&config)), None);
        restore("ATM_IDENTITY", original_identity);
    }

    #[test]
    #[serial_test::serial]
    fn team_resolution_prefers_flag_then_env_then_config() {
        let original_team = env::var_os("ATM_TEAM");
        set_env_var("ATM_TEAM", "env-team");

        let config = AtmConfig {
            default_team: Some("config-team".into()),
            ..Default::default()
        };

        assert_eq!(
            resolve_team(Some("flag-team"), Some(&config)).as_deref(),
            Some("flag-team")
        );
        assert_eq!(
            resolve_team(None, Some(&config)).as_deref(),
            Some("env-team")
        );

        remove_env_var("ATM_TEAM");
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

    fn temp_config_path() -> PathBuf {
        env::temp_dir().join("config.json")
    }

    fn restore(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => set_env_var(key, value),
            None => remove_env_var(key),
        }
    }

    fn set_env_var<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(key: K, value: V) {
        // SAFETY: these tests use serial execution before mutating process
        // environment variables, so there is no concurrent access in this
        // process while the mutation is performed.
        unsafe { env::set_var(key, value) }
    }

    fn remove_env_var<K: AsRef<std::ffi::OsStr>>(key: K) {
        // SAFETY: these tests use serial execution before mutating process
        // environment variables, so there is no concurrent access in this
        // process while the mutation is performed.
        unsafe { env::remove_var(key) }
    }
}
