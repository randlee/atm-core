//! ATM config discovery, loading, normalization, and team-config parsing.
//!
//! # Deprecated
//!
//! `[atm].identity` and the legacy top-level `identity` key remain
//! compatibility-only parsing inputs so ATM can emit
//! `ATM_WARNING_IDENTITY_DRIFT` for obsolete configs. They no longer control
//! runtime sender identity resolution. Set `ATM_IDENTITY` instead and remove
//! the deprecated config keys once the environment-based identity is in place.

pub mod aliases;
pub mod bridge;
pub mod discovery;
pub mod types;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use toml::Value as TomlValue;
use tracing::warn;

pub use types::AtmConfig;

use crate::error::{AtmError, AtmErrorCode, AtmErrorKind};
use crate::schema::{AgentMember, TeamConfig};
use crate::types::{AgentName, TeamName};
use discovery::normalize_post_send_hooks;
use types::{ByteCount, GraftConfig, MAX_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES, MAX_POST_SEND_HOOKS};

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
    let parsed = parse_raw_config_file(&path)?;
    let obsolete_identity_present = parsed.atm.identity.is_some() || parsed.identity.is_some();
    let config_root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    validate_post_send_hook_count(&parsed.atm.post_send_hooks, &path)?;

    Ok(Some(AtmConfig {
        identity: parsed.atm.identity.or(parsed.identity),
        default_team: parse_default_team(parsed.atm.default_team.or(parsed.default_team), &path)?,
        team_members: normalize_team_members(parsed.atm.team_members, &path)?,
        aliases: normalize_aliases(parsed.atm.aliases),
        post_send_hooks: normalize_post_send_hooks(parsed.atm.post_send_hooks, &config_root)?,
        claude_jsonl_body_export_max_bytes: normalize_claude_jsonl_body_export_max_bytes(
            parsed.atm.claude_jsonl_body_export_max_bytes,
            &path,
        )?,
        daemon: parse_daemon_config(&parsed.daemon, &path)?,
        graft: normalize_graft_config(parsed.atm.graft),
        config_root,
        obsolete_identity_present,
    }))
}

fn parse_raw_config_file(path: &Path) -> Result<RawConfigFile, AtmError> {
    let contents = fs::read_to_string(path).map_err(|error| {
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
    reject_legacy_post_send_hook_keys(path, &raw_toml)?;
    raw_toml.try_into::<RawConfigFile>().map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!("failed to parse config at {}: {error}", path.display()),
        )
        .with_recovery(
            "Repair the .atm.toml syntax or remove malformed ATM config keys before retrying.",
        )
        .with_source(error)
    })
}

fn parse_default_team(raw_team: Option<String>, path: &Path) -> Result<Option<TeamName>, AtmError> {
    raw_team
        .map(|team| {
            team.parse::<TeamName>().map_err(|error| {
                AtmError::new(
                    AtmErrorKind::Config,
                    format!("invalid default team in {}: {}", path.display(), error.message),
                )
                .with_recovery(
                    "Use a valid ATM team name in [atm].default_team or default_team without path separators or surrounding whitespace.",
                )
            })
        })
        .transpose()
}

fn parse_daemon_config(
    daemon: &RawDaemonSection,
    path: &Path,
) -> Result<types::DaemonConfig, AtmError> {
    let config = daemon
        .remote_retry_budget
        .as_deref()
        .map(|value| parse_duration_literal(value, path, "daemon.remote_retry_budget"))
        .transpose()?
        .map(|remote_retry_budget| types::DaemonConfig {
            remote_retry_budget,
        })
        .unwrap_or_default();
    Ok(config)
}

/// Load and validate the Claude Code `config.json` document for one team
/// directory.
///
/// This parser is intentionally narrower than generic runtime roster lookups:
/// production callers should use it only from approved comparison, ingest, or
/// shell-preservation seams.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::ConfigTeamMissing`] when the team config
/// document does not exist, or
/// [`crate::error_codes::AtmErrorCode::ConfigTeamParseFailed`] when the JSON
/// document is malformed or violates the required team-config shape.
pub fn load_claude_team_config_document(team_dir: &Path) -> Result<TeamConfig, AtmError> {
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
/// The `_config` parameter is retained only to preserve the shared config-aware
/// helper signature used across command code paths. Identity is resolved
/// exclusively via the `ATM_IDENTITY` environment variable and will never fall
/// back to deprecated config identity fields.
pub fn resolve_identity(_config: Option<&AtmConfig>) -> Option<AgentName> {
    env::var("ATM_IDENTITY")
        .ok()
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
}

/// Resolve the active team from explicit override, environment, or config.
pub fn resolve_team(team_override: Option<&str>, config: Option<&AtmConfig>) -> Option<TeamName> {
    team_override
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            env::var("ATM_TEAM")
                .ok()
                .filter(|value| !value.is_empty())
                .and_then(|value| value.parse().ok())
        })
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
    daemon: RawDaemonSection,
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
    post_send_hooks: Vec<RawPostSendHookRule>,
    #[serde(default)]
    claude_jsonl_body_export_max_bytes: Option<u64>,
    #[serde(default)]
    graft: Option<RawGraftSection>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDaemonSection {
    #[serde(default)]
    remote_retry_budget: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawGraftSection {
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPostSendHookRule {
    recipient: String,
    command: Vec<String>,
}

fn reject_legacy_post_send_hook_keys(path: &Path, raw_toml: &TomlValue) -> Result<(), AtmError> {
    let Some(atm) = raw_toml.get("atm").and_then(TomlValue::as_table) else {
        return Ok(());
    };

    let retired_present = atm.contains_key("post_send_hook_members");
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
            "Replace 'post_send_hook_members' with one or more [[atm.post_send_hooks]] rules, each containing recipient = \"name-or-*\" and command = [\"argv\", ...].",
        ));
    }

    let legacy_shape_present = atm.contains_key("post_send_hook")
        || atm.contains_key("post_send_hook_senders")
        || atm.contains_key("post_send_hook_recipients");
    if legacy_shape_present {
        return Err(AtmError::new_with_code(
            AtmErrorCode::ConfigRetiredLegacyHookKeys,
            AtmErrorKind::Config,
            format!(
                "error: '{}' uses retired post-send hook keys. Use [[atm.post_send_hooks]] with recipient and command entries instead.",
                path.display()
            ),
        )
        .with_recovery(
            "Replace [atm].post_send_hook, [atm].post_send_hook_senders, and [atm].post_send_hook_recipients with one or more [[atm.post_send_hooks]] rules, each containing recipient = \"name-or-*\" and command = [\"argv\", ...].",
        ));
    }
    Ok(())
}

fn normalize_claude_jsonl_body_export_max_bytes(
    raw: Option<u64>,
    path: &Path,
) -> Result<ByteCount, AtmError> {
    let value = raw.unwrap_or(types::DEFAULT_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES);
    if value > MAX_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES {
        return Err(AtmError::new_with_code(
            AtmErrorCode::ConfigParseFailed,
            AtmErrorKind::Config,
            format!(
                "invalid [atm].claude_jsonl_body_export_max_bytes in {}: {value} exceeds the maximum of {} bytes",
                path.display(),
                MAX_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES
            ),
        )
        .with_recovery(
            "Set [atm].claude_jsonl_body_export_max_bytes to a value between 0 and 1048576 bytes before retrying.",
        ));
    }
    Ok(ByteCount::new(value))
}

fn validate_post_send_hook_count(
    post_send_hooks: &[RawPostSendHookRule],
    path: &Path,
) -> Result<(), AtmError> {
    if post_send_hooks.len() > MAX_POST_SEND_HOOKS {
        return Err(AtmError::new_with_code(
            AtmErrorCode::ConfigParseFailed,
            AtmErrorKind::Config,
            format!(
                "invalid [[atm.post_send_hooks]] count in {}: {} exceeds the maximum of {}",
                path.display(),
                post_send_hooks.len(),
                MAX_POST_SEND_HOOKS
            ),
        )
        .with_recovery("Reduce [[atm.post_send_hooks]] entries to 64 or fewer before retrying."));
    }
    Ok(())
}

fn normalize_graft_config(raw: Option<RawGraftSection>) -> GraftConfig {
    GraftConfig {
        enabled: raw.and_then(|section| section.enabled).unwrap_or(true),
    }
}

fn parse_duration_literal(
    value: &str,
    path: &Path,
    field_name: &str,
) -> Result<Duration, AtmError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(duration_parse_error(
            path,
            field_name,
            "value must not be blank".to_string(),
            "Use one positive duration literal such as 30s, 500ms, or 2m for daemon timeout settings.",
        ));
    }

    let (number, unit) = parse_duration_parts(trimmed, path, field_name)?;
    let amount = parse_duration_amount(number, path, field_name)?;
    if amount == 0 {
        return Err(duration_parse_error(
            path,
            field_name,
            "zero is not allowed".to_string(),
            "Use one positive duration literal such as 30s, 500ms, or 2m for daemon timeout settings.",
        ));
    }

    Ok(duration_from_unit(amount, unit))
}

fn parse_duration_parts<'a>(
    trimmed: &'a str,
    path: &Path,
    field_name: &str,
) -> Result<(&'a str, &'static str), AtmError> {
    if let Some(value) = trimmed.strip_suffix("ms") {
        return Ok((value, "ms"));
    }
    if let Some(value) = trimmed.strip_suffix('s') {
        return Ok((value, "s"));
    }
    if let Some(value) = trimmed.strip_suffix('m') {
        return Ok((value, "m"));
    }
    Err(duration_parse_error(
        path,
        field_name,
        format!("unsupported unit '{trimmed}'"),
        "Use one positive duration literal ending in ms, s, or m for daemon timeout settings.",
    ))
}

fn parse_duration_amount(number: &str, path: &Path, field_name: &str) -> Result<u64, AtmError> {
    number.parse::<u64>().map_err(|error| {
        duration_parse_error_with_source(
            path,
            field_name,
            error.to_string(),
            "Use one positive integer duration literal such as 30s, 500ms, or 2m for daemon timeout settings.",
            error,
        )
    })
}

fn duration_from_unit(amount: u64, unit: &str) -> Duration {
    match unit {
        "ms" => Duration::from_millis(amount),
        "s" => Duration::from_secs(amount),
        "m" => Duration::from_secs(amount.saturating_mul(60)),
        _ => unreachable!("duration unit filtered above"),
    }
}

fn duration_parse_error(
    path: &Path,
    field_name: &str,
    detail: String,
    recovery: &'static str,
) -> AtmError {
    AtmError::new_with_code(
        AtmErrorCode::ConfigParseFailed,
        AtmErrorKind::Config,
        format!(
            "invalid duration for {} in {}: {}",
            field_name,
            path.display(),
            detail
        ),
    )
    .with_recovery(recovery)
}

fn duration_parse_error_with_source(
    path: &Path,
    field_name: &str,
    detail: String,
    recovery: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> AtmError {
    duration_parse_error(path, field_name, detail, recovery).with_source(source)
}

fn normalize_team_members(values: Vec<String>, path: &Path) -> Result<Vec<TeamName>, AtmError> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<TeamName>().map_err(|error| {
                AtmError::new(
                    AtmErrorKind::Config,
                    format!("invalid [atm].team_members entry in {}: {error}", path.display()),
                )
                .with_recovery(
                    "Use valid ATM team-member names in [atm].team_members without path separators or surrounding whitespace.",
                )
            })
        })
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
        Value::String(name) => match name.parse::<AgentName>() {
            Ok(name) => Some(AgentMember::with_name(name)),
            Err(error) => {
                warn!(
                    code = %AtmErrorCode::WarningInvalidTeamMemberSkipped,
                    path = %config_path.display(),
                    member_index = index,
                    member = %name,
                    %error,
                    "skipping invalid team member record"
                );
                None
            }
        },
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
    use crate::config::types::{
        ByteCount, HookRecipient, MAX_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES, MAX_POST_SEND_HOOKS,
    };
    use crate::error_codes::AtmErrorCode;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::test_support::{TEST_QA, TEST_SENDER, TEST_TEAM};
    use crate::types::TeamName;
    use serde_json::Value;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    use super::{AtmConfig, load_config, parse_team_config, resolve_identity, resolve_team};

    #[test]
    fn load_config_walks_upward_for_dot_atm_toml() {
        let root = unique_temp_dir("config-discovery");
        let nested = root.path().join("workspace").join("nested");
        fs::create_dir_all(&nested).expect("nested dir");
        fs::write(
            root.path().join(".atm.toml"),
            format!("[atm]\nidentity = \"{TEST_SENDER}\"\ndefault_team = \"{TEST_TEAM}\"\n"),
        )
        .expect("config");

        let config = load_config(&nested).expect("config").expect("present");
        assert_eq!(config.identity.as_deref(), Some(TEST_SENDER));
        assert_eq!(config.default_team.as_deref(), Some(TEST_TEAM));
        assert_eq!(config.config_root, root.path());
        assert!(config.obsolete_identity_present);
    }

    #[test]
    fn load_config_accepts_legacy_top_level_keys_for_compatibility() {
        let root = unique_temp_dir("legacy-config");
        fs::write(
            root.path().join(".atm.toml"),
            format!("identity = \"{TEST_SENDER}\"\ndefault_team = \"{TEST_TEAM}\"\n"),
        )
        .expect("config");

        let config = load_config(root.path()).expect("config").expect("present");
        assert_eq!(config.identity.as_deref(), Some(TEST_SENDER));
        assert_eq!(config.default_team.as_deref(), Some(TEST_TEAM));
        assert_eq!(config.config_root, root.path());
        assert!(config.obsolete_identity_present);
    }

    #[test]
    fn load_config_reads_team_members_aliases_and_post_send_hooks() {
        let root = unique_temp_dir("atm-config-surface");
        fs::write(
            root.path().join(".atm.toml"),
            format!(
                r#"[atm]
default_team = "{TEST_TEAM}"
team_members = ["{ROLE_TEAM_LEAD}", "{TEST_SENDER}", " ", "qa"]

[[atm.post_send_hooks]]
recipient = "{ROLE_TEAM_LEAD}"
command = ["scripts/atm-nudge.sh", "{ROLE_TEAM_LEAD}"]

[[atm.post_send_hooks]]
recipient = "*"
command = ["bash", "-lc", "echo hi"]

[daemon]
remote_retry_budget = "45s"

[atm.aliases]
tl = "{ROLE_TEAM_LEAD}"
qa = "{TEST_QA}"
blank = ""
"#,
            ),
        )
        .expect("config");

        let config = load_config(root.path()).expect("config").expect("present");
        assert_eq!(
            config.team_members,
            vec![
                ROLE_TEAM_LEAD.parse::<TeamName>().expect("team member"),
                TEST_SENDER.parse::<TeamName>().expect("team member"),
                "qa".parse::<TeamName>().expect("team member"),
            ]
        );
        assert_eq!(config.post_send_hooks.len(), 2);
        assert_eq!(
            config.post_send_hooks[0].recipient,
            HookRecipient::Named(ROLE_TEAM_LEAD.parse().expect("recipient"))
        );
        assert_eq!(
            config.post_send_hooks[0].command,
            vec![
                root.path()
                    .join("scripts/atm-nudge.sh")
                    .display()
                    .to_string(),
                ROLE_TEAM_LEAD.to_string()
            ]
        );
        assert_eq!(config.post_send_hooks[1].recipient, HookRecipient::Wildcard);
        assert_eq!(
            config.post_send_hooks[1].command,
            vec!["bash".to_string(), "-lc".to_string(), "echo hi".to_string()]
        );
        assert_eq!(
            config.claude_jsonl_body_export_max_bytes,
            ByteCount::new(128 * 1024)
        );
        assert_eq!(
            config.aliases.get("tl").map(String::as_str),
            Some(ROLE_TEAM_LEAD)
        );
        assert_eq!(config.aliases.get("qa").map(String::as_str), Some(TEST_QA));
        assert!(!config.aliases.contains_key("blank"));
        assert_eq!(
            config.daemon.remote_retry_budget,
            std::time::Duration::from_secs(45)
        );
    }

    #[test]
    fn load_config_rejects_invalid_remote_retry_budget() {
        let root = unique_temp_dir("atm-config-invalid-remote-retry-budget");
        fs::write(
            root.path().join(".atm.toml"),
            "[daemon]\nremote_retry_budget = \"soon\"\n",
        )
        .expect("config");

        let error = load_config(root.path()).expect_err("invalid duration");

        assert_eq!(error.code, AtmErrorCode::ConfigParseFailed);
        assert!(error.message.contains("daemon.remote_retry_budget"));
    }

    #[test]
    fn load_config_reads_jsonl_body_export_cap_and_allows_zero() {
        let root = unique_temp_dir("atm-config-jsonl-cap");
        fs::write(
            root.path().join(".atm.toml"),
            "[atm]\nclaude_jsonl_body_export_max_bytes = 0\n",
        )
        .expect("config");

        let config = load_config(root.path()).expect("config").expect("present");

        assert_eq!(config.claude_jsonl_body_export_max_bytes, ByteCount::new(0));
    }

    #[test]
    fn load_config_reads_graft_enabled_and_defaults_true() {
        let disabled_root = unique_temp_dir("atm-config-graft-disabled");
        fs::write(
            disabled_root.path().join(".atm.toml"),
            "[atm.graft]\nenabled = false\n",
        )
        .expect("config");

        let disabled = load_config(disabled_root.path())
            .expect("config")
            .expect("present");
        assert!(!disabled.graft.enabled);

        let default_root = unique_temp_dir("atm-config-graft-default");
        fs::write(default_root.path().join(".atm.toml"), "[atm]\n").expect("config");

        let default_config = load_config(default_root.path())
            .expect("config")
            .expect("present");
        assert!(default_config.graft.enabled);
    }

    #[test]
    fn load_config_rejects_jsonl_body_export_cap_above_maximum() {
        let root = unique_temp_dir("atm-config-jsonl-cap-too-large");
        fs::write(
            root.path().join(".atm.toml"),
            format!(
                "[atm]\nclaude_jsonl_body_export_max_bytes = {}\n",
                MAX_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES + 1
            ),
        )
        .expect("config");

        let error = load_config(root.path()).expect_err("oversized export cap should fail");

        assert_eq!(error.code, AtmErrorCode::ConfigParseFailed);
        assert!(error.message.contains("claude_jsonl_body_export_max_bytes"));
        assert!(error.message.contains("exceeds the maximum"));
    }

    #[test]
    fn load_config_rejects_too_many_post_send_hooks() {
        let root = unique_temp_dir("atm-config-too-many-post-send-hooks");
        let mut config = String::from("[atm]\n");
        for index in 0..=MAX_POST_SEND_HOOKS {
            config.push_str("[[atm.post_send_hooks]]\n");
            config.push_str("recipient = \"*\"\n");
            config.push_str(&format!("command = [\"hook-{index}\"]\n\n"));
        }
        fs::write(root.path().join(".atm.toml"), config).expect("config");

        let error = load_config(root.path()).expect_err("too many hooks should fail");

        assert_eq!(error.code, AtmErrorCode::ConfigParseFailed);
        assert!(error.message.contains("[[atm.post_send_hooks]]"));
        assert!(error.message.contains("exceeds the maximum"));
    }

    #[test]
    fn load_config_rejects_invalid_team_member_name() {
        let root = unique_temp_dir("atm-config-invalid-team-member");
        fs::write(
            root.path().join(".atm.toml"),
            format!("[atm]\nteam_members = [\"{ROLE_TEAM_LEAD}\", \"bad/name\"]\n"),
        )
        .expect("config");

        let error = load_config(root.path()).expect_err("invalid team member");

        assert!(error.message.contains("[atm].team_members"));
    }

    #[test]
    fn load_config_ignores_core_section_hook_keys() {
        let root = unique_temp_dir("core-config-hook-keys");
        fs::write(
            root.path().join(".atm.toml"),
            format!(
                r#"[core]
default_team = "{TEST_TEAM}"
identity = "{ROLE_TEAM_LEAD}"

[[atm.post_send_hooks]]
recipient = "{TEST_SENDER}"
command = ["scripts/atm-nudge.sh", "{TEST_SENDER}"]
"#
            ),
        )
        .expect("config");

        let config = load_config(root.path()).expect("config").expect("present");
        assert_eq!(config.default_team, None);
        assert_eq!(config.identity, None);
        assert_eq!(config.post_send_hooks.len(), 1);
        assert!(!config.obsolete_identity_present);
    }

    #[test]
    fn load_config_does_not_fall_back_to_process_cwd() {
        let root = unique_temp_dir("config-ancestor-chain");
        let nested = root.path().join("nested").join("cwd");
        fs::create_dir_all(&nested).expect("nested cwd");

        let ambient = unique_temp_dir("config-process-cwd");
        fs::write(
            ambient.path().join(".atm.toml"),
            format!("[atm]\nidentity = \"{ROLE_TEAM_LEAD}\"\n"),
        )
        .expect("ambient config");

        let original_cwd = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(ambient.path()).expect("set ambient cwd");
        let loaded = load_config(&nested).expect("config lookup should not fail");
        std::env::set_current_dir(original_cwd).expect("restore cwd");

        assert!(
            loaded.is_none(),
            "config lookup must stay on the provided ancestor chain and never fall back to process cwd"
        );
    }

    #[test]
    fn load_config_rejects_retired_post_send_hook_members_key() {
        let root = unique_temp_dir("retired-hook-members");
        fs::write(
            root.path().join(".atm.toml"),
            format!(
                r#"[atm]
post_send_hook_members = ["{ROLE_TEAM_LEAD}"]
"#
            ),
        )
        .expect("config");

        let error = load_config(root.path()).expect_err("retired key should fail");

        assert!(error.is_config());
        assert_eq!(error.code, AtmErrorCode::ConfigRetiredHookMembersKey);
        assert!(
            error
                .message
                .contains(&root.path().join(".atm.toml").display().to_string())
        );
        assert!(error.message.contains("post_send_hook_members"));
        assert_eq!(
            error.recovery.as_deref(),
            Some(
                "Replace 'post_send_hook_members' with one or more [[atm.post_send_hooks]] rules, each containing recipient = \"name-or-*\" and command = [\"argv\", ...]."
            )
        );
    }

    #[test]
    fn load_config_rejects_legacy_post_send_filter_keys() {
        let root = unique_temp_dir("legacy-hook-filters");
        fs::write(
            root.path().join(".atm.toml"),
            format!(
                r#"[atm]
post_send_hook = ["bin/hook"]
post_send_hook_recipients = ["{ROLE_TEAM_LEAD}"]
"#
            ),
        )
        .expect("config");

        let error = load_config(root.path()).expect_err("legacy hook shape should fail");

        assert!(error.is_config());
        assert_eq!(error.code, AtmErrorCode::ConfigRetiredLegacyHookKeys);
        assert!(error.message.contains("retired post-send hook keys"));
        assert!(error.message.contains("[[atm.post_send_hooks]]"));
        assert_eq!(
            error.recovery.as_deref(),
            Some(
                "Replace [atm].post_send_hook, [atm].post_send_hook_senders, and [atm].post_send_hook_recipients with one or more [[atm.post_send_hooks]] rules, each containing recipient = \"name-or-*\" and command = [\"argv\", ...]."
            )
        );
    }

    #[test]
    fn parse_team_config_accepts_object_members() {
        let (_tempdir, config_path) = temp_config_path();
        let raw =
            format!(r#"{{"members":[{{"name":"{TEST_SENDER}"}},{{"name":"{ROLE_TEAM_LEAD}"}}]}}"#);
        let config = parse_team_config(&config_path, &raw).expect("team config");

        assert_eq!(config.members.len(), 2);
        assert_eq!(config.members[0].name, TEST_SENDER);
        assert_eq!(config.members[1].name, ROLE_TEAM_LEAD);
        assert!(config.extra.is_empty());
    }

    #[test]
    fn parse_team_config_accepts_string_member_compatibility() {
        let (_tempdir, config_path) = temp_config_path();
        let raw = format!(r#"{{"members":["{TEST_SENDER}",{{"name":"{ROLE_TEAM_LEAD}"}}]}}"#);
        let config = parse_team_config(&config_path, &raw).expect("team config");

        assert_eq!(config.members.len(), 2);
        assert_eq!(config.members[0].name, TEST_SENDER);
        assert_eq!(config.members[1].name, ROLE_TEAM_LEAD);
        assert!(config.extra.is_empty());
    }

    #[test]
    fn parse_team_config_skips_invalid_member_records() {
        let (_tempdir, config_path) = temp_config_path();
        let raw = format!(
            r#"{{"members":[{{"name":"{TEST_SENDER}"}},{{"broken":true}},17,{{"name":"{ROLE_TEAM_LEAD}"}}]}}"#
        );
        let config = parse_team_config(&config_path, &raw).expect("team config");

        assert_eq!(config.members.len(), 2);
        assert_eq!(config.members[0].name, TEST_SENDER);
        assert_eq!(config.members[1].name, ROLE_TEAM_LEAD);
        assert!(config.extra.is_empty());
    }

    #[test]
    fn parse_team_config_defaults_missing_members_to_empty() {
        let (_tempdir, config_path) = temp_config_path();
        let config = parse_team_config(&config_path, r#"{}"#).expect("team config");

        assert!(config.members.is_empty());
        assert!(config.extra.is_empty());
    }

    #[test]
    fn parse_team_config_preserves_root_extra_fields() {
        let (_tempdir, config_path) = temp_config_path();
        let raw =
            format!(r#"{{"leadSessionId":"lead-123","members":[{{"name":"{ROLE_TEAM_LEAD}"}}]}}"#);
        let config = parse_team_config(&config_path, &raw).expect("team config");

        assert_eq!(config.members.len(), 1);
        assert_eq!(
            config.extra["leadSessionId"],
            Value::String("lead-123".to_string())
        );
    }

    #[test]
    fn parse_team_config_reports_json_syntax_errors_with_detail() {
        let (_tempdir, config_path) = temp_config_path();
        let raw = format!(r#"{{"members":[{{"name":"{TEST_SENDER}""#);
        let error = parse_team_config(&config_path, &raw).expect_err("syntax error");

        assert!(error.is_config());
        assert_eq!(error.code, AtmErrorCode::ConfigTeamParseFailed);
        assert!(error.message.contains("config.json"));
        assert!(error.message.contains("EOF while parsing"));
        assert!(error.recovery.as_deref().is_some());
    }

    #[test]
    fn parse_team_config_rejects_non_object_root() {
        let (_tempdir, config_path) = temp_config_path();
        let raw = format!(r#"["{TEST_SENDER}"]"#);
        let error = parse_team_config(&config_path, &raw).expect_err("root shape error");

        assert!(error.is_config());
        assert_eq!(error.code, AtmErrorCode::ConfigTeamParseFailed);
        assert!(error.message.contains("root value must be a JSON object"));
        assert!(error.recovery.as_deref().is_some());
    }

    #[test]
    fn parse_team_config_rejects_non_array_members() {
        let (_tempdir, config_path) = temp_config_path();
        let raw = format!(r#"{{"members":{{"name":"{TEST_SENDER}"}}}}"#);
        let error = parse_team_config(&config_path, &raw).expect_err("members shape error");

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
        let team_dir = root.path().join("team");
        fs::create_dir_all(&team_dir).expect("team dir");

        let error = super::load_claude_team_config_document(&team_dir).expect_err("missing config");

        assert!(error.is_missing_document());
        assert!(error.message.contains("team config is missing"));
        assert!(error.recovery.as_deref().is_some());
    }

    #[test]
    #[serial_test::serial(env)]
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
    #[serial_test::serial(env)]
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
    #[serial_test::serial(env)]
    fn team_resolution_prefers_flag_then_env_then_config() {
        let original_team = env::var_os("ATM_TEAM");
        set_env_var("ATM_TEAM", "env-team");

        let config = AtmConfig {
            default_team: Some("config-team".parse().expect("team")),
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

    fn unique_temp_dir(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(label)
            .tempdir()
            .expect("temp dir")
    }

    fn temp_config_path() -> (tempfile::TempDir, PathBuf) {
        let tempdir = tempdir().expect("tempdir");
        let root = tempdir.path().to_path_buf();
        let nested = root.join("atm config root").join("nested config dir");
        fs::create_dir_all(&nested).expect("nested config dir");
        (tempdir, nested.join("config.json"))
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
