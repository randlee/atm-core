use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use tracing::warn;

use crate::address::validate_path_segment;
use crate::config::{load_config, load_team_config, resolve_team};
use crate::error::{AtmError, AtmErrorCode, AtmErrorKind};
use crate::home;
use crate::persistence;
use crate::schema::{AgentMember, TeamConfig};
use crate::types::{AgentName, TeamName};

#[path = "team_admin/restore.rs"]
mod restore;

/// One discovered team and its current member count.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TeamSummary {
    pub name: TeamName,
    pub member_count: usize,
}

/// Result of listing discoverable teams under ATM home.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TeamsList {
    pub action: String,
    pub team: TeamName,
    pub teams: Vec<TeamSummary>,
}

/// One member entry from a team's live `config.json` roster.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MemberSummary {
    pub name: AgentName,
    pub agent_id: String,
    pub agent_type: String,
    pub model: String,
    pub joined_at: Option<u64>,
    pub tmux_pane_id: String,
    pub cwd: String,
    pub extra: serde_json::Map<String, Value>,
}

/// Result of listing all current members for one team.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MembersList {
    pub team: TeamName,
    pub members: Vec<MemberSummary>,
}

/// Parameters for listing the members of one team.
#[derive(Debug, Clone)]
pub struct MembersQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub team_override: Option<TeamName>,
}

/// Parameters for adding one member to a team roster.
#[derive(Debug, Clone)]
pub struct AddMemberRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub member: AgentName,
    pub agent_type: String,
    pub model: String,
    pub cwd: PathBuf,
    pub tmux_pane_id: Option<String>,
}

impl AddMemberRequest {
    pub fn new(
        home_dir: PathBuf,
        team: &str,
        member: &str,
        agent_type: String,
        model: String,
        cwd: PathBuf,
        tmux_pane_id: Option<String>,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            home_dir,
            team: team.parse()?,
            member: member.parse()?,
            agent_type,
            model,
            cwd,
            tmux_pane_id,
        })
    }
}

/// Result of adding one member and optional inbox to a team.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AddMemberOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub member: AgentName,
    pub created_inbox: bool,
}

/// Parameters for creating one team backup.
#[derive(Debug, Clone)]
pub struct BackupRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
}

impl BackupRequest {
    pub fn new(home_dir: PathBuf, team: &str) -> Result<Self, AtmError> {
        Ok(Self {
            home_dir,
            team: team.parse()?,
        })
    }
}

/// Result of one successful team backup.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BackupOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub backup_path: PathBuf,
}

/// Parameters for restoring one team from backup.
#[derive(Debug, Clone)]
pub struct RestoreRequest {
    pub home_dir: PathBuf,
    pub team: TeamName,
    pub from: Option<PathBuf>,
    pub dry_run: bool,
}

impl RestoreRequest {
    pub fn new(
        home_dir: PathBuf,
        team: &str,
        from: Option<PathBuf>,
        dry_run: bool,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            home_dir,
            team: team.parse()?,
            from,
            dry_run,
        })
    }
}

/// Dry-run restore plan for one backup restore attempt.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RestorePlan {
    pub action: &'static str,
    pub team: TeamName,
    pub backup_path: PathBuf,
    pub dry_run: bool,
    pub would_restore_members: Vec<AgentName>,
    pub would_restore_inboxes: Vec<String>,
    pub would_restore_tasks: usize,
}

/// Applied restore summary for one team restore operation.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub backup_path: PathBuf,
    pub members_restored: usize,
    pub inboxes_restored: usize,
    pub tasks_restored: usize,
}

/// Result of a restore command, either as a dry-run plan or applied change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreResult {
    DryRun(RestorePlan),
    Applied(RestoreOutcome),
}

/// List teams currently discoverable under ATM home.
///
/// # Errors
///
/// Returns [`AtmError`] when `.atm.toml` cannot be loaded or the teams root
/// cannot be enumerated.
pub fn list_teams(home_dir: PathBuf, current_dir: PathBuf) -> Result<TeamsList, AtmError> {
    let config = load_config(&current_dir)?;
    let current_team = resolve_team(None, config.as_ref()).unwrap_or_default();
    let teams_root = teams_root_from_home(&home_dir);
    if !teams_root.exists() {
        return Ok(TeamsList {
            action: "list".to_string(),
            team: current_team,
            teams: Vec::new(),
        });
    }

    let mut teams = Vec::new();
    for entry in fs::read_dir(&teams_root).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to read teams directory {}: {error}",
            teams_root.display()
        ))
        .with_source(error)
        .with_recovery("Check ATM_HOME and ensure the teams directory is readable.")
    })? {
        let entry = entry.map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read teams directory entry under {}: {error}",
                teams_root.display()
            ))
            .with_source(error)
            .with_recovery("Check ATM_HOME and ensure the teams directory is readable.")
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if !path.join("config.json").is_file() {
            continue;
        }

        match load_team_config(&path) {
            Ok(config) => teams.push(TeamSummary {
                name: TeamName::from_validated(entry.file_name().to_string_lossy().to_string()),
                member_count: config.members.len(),
            }),
            Err(error) => warn!(
                code = %AtmErrorCode::ConfigTeamParseFailed,
                path = %path.display(),
                %error,
                "skipping malformed team config while listing teams"
            ),
        }
    }

    teams.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(TeamsList {
        action: "list".to_string(),
        team: current_team,
        teams,
    })
}

/// List the current member roster for one team.
///
/// # Errors
///
/// Returns [`AtmError`] when team resolution fails, the team directory is
/// missing, or `config.json` cannot be loaded.
pub fn list_members(query: MembersQuery) -> Result<MembersList, AtmError> {
    let config = load_config(&query.current_dir)?;
    let team = resolve_team(query.team_override.as_deref(), config.as_ref())
        .ok_or_else(AtmError::team_unavailable)?;
    let team_dir = home::team_dir_from_home(&query.home_dir, &team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&team));
    }
    let config = load_team_config(&team_dir)?;

    let mut members = Vec::with_capacity(config.members.len());
    if let Some(team_lead) = config
        .members
        .iter()
        .find(|member| member.name == "team-lead")
    {
        members.push(member_summary(team_lead));
    }
    for member in &config.members {
        if member.name == "team-lead" {
            continue;
        }
        members.push(member_summary(member));
    }

    Ok(MembersList { team, members })
}

/// Add one member record and inbox file to a team.
///
/// # Errors
///
/// Returns [`AtmError`] when the team is missing, the member already exists, or
/// inbox/config persistence fails.
pub fn add_member(request: AddMemberRequest) -> Result<AddMemberOutcome, AtmError> {
    let team_dir = home::team_dir_from_home(&request.home_dir, &request.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&request.team));
    }

    let mut config = load_team_config(&team_dir)?;
    if config
        .members
        .iter()
        .any(|member| member.name == request.member.as_str())
    {
        return Err(AtmError::validation(format!(
            "member '{}' already exists in team '{}'",
            request.member, request.team
        )));
    }

    let inbox_path = home::inbox_path_from_home(&request.home_dir, &request.team, &request.member)?;
    let created_inbox = ensure_inbox_exists(&inbox_path)?;

    let normalized_tmux_pane_id = normalize_tmux_pane_id(request.tmux_pane_id.as_deref())?;
    let mut extra = serde_json::Map::new();
    if normalized_tmux_pane_id.is_some() {
        extra.insert("backendType".to_string(), json!("tmux"));
        extra.insert("isActive".to_string(), json!(true));
    }

    config.members.push(AgentMember {
        name: request.member.to_string(),
        agent_id: format!("{}@{}", request.member, request.team),
        agent_type: request.agent_type,
        model: request.model,
        joined_at: Some(Utc::now().timestamp_millis() as u64),
        tmux_pane_id: normalized_tmux_pane_id.unwrap_or_default(),
        cwd: request.cwd.display().to_string(),
        extra,
    });

    if let Err(error) = write_team_config(&team_dir, &config) {
        if created_inbox {
            let _ = fs::remove_file(&inbox_path);
        }
        return Err(
            error.with_recovery("Check team config permissions and rerun `atm teams add-member`.")
        );
    }

    Ok(AddMemberOutcome {
        action: "add-member",
        team: request.team,
        member: request.member,
        created_inbox,
    })
}

/// Create a point-in-time backup of one team's config, inboxes, and task files.
///
/// # Errors
///
/// Returns [`AtmError`] when the team/config is missing or backup directory/file
/// creation fails.
pub fn backup_team(request: BackupRequest) -> Result<BackupOutcome, AtmError> {
    let team_dir = home::team_dir_from_home(&request.home_dir, &request.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&request.team));
    }

    let config_path = team_dir.join("config.json");
    if !config_path.is_file() {
        return Err(AtmError::missing_document(format!(
            "team config is missing at {}",
            config_path.display()
        )));
    }

    let backup_dir = backup_root_from_home(&request.home_dir, &request.team)?.join(timestamp_dir());
    fs::create_dir_all(backup_dir.join("inboxes")).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to create backup directory {}: {error}",
            backup_dir.display()
        ))
        .with_source(error)
        .with_recovery("Check backup directory permissions under ATM_HOME and retry the backup.")
    })?;

    fs::copy(&config_path, backup_dir.join("config.json")).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to copy {} into backup {}: {error}",
            config_path.display(),
            backup_dir.display()
        ))
        .with_source(error)
        .with_recovery("Check source and backup directory permissions and retry the backup.")
    })?;

    copy_regular_files(
        &team_dir.join("inboxes"),
        &backup_dir.join("inboxes"),
        |name| !name.starts_with('.') && !name.ends_with(".lock"),
    )?;
    copy_regular_files(
        &tasks_dir_from_home(&request.home_dir, &request.team)?,
        &backup_dir.join("tasks"),
        |name| name == ".highwatermark" || name.ends_with(".json"),
    )?;

    Ok(BackupOutcome {
        action: "backup",
        team: request.team,
        backup_path: backup_dir,
    })
}

/// Restore one team from a backup directory.
///
/// # Errors
///
/// Returns [`AtmError`] when backup discovery, staging/live restore work, or
/// config-last persistence fails. Failure to remove the restore marker after a
/// successful restore is degraded to a warning-only follow-up path.
pub fn restore_team(request: RestoreRequest) -> Result<RestoreResult, AtmError> {
    restore::restore_team(request)
}

fn member_summary(member: &AgentMember) -> MemberSummary {
    MemberSummary {
        name: AgentName::from_validated(member.name.clone()),
        agent_id: member.agent_id.clone(),
        agent_type: member.agent_type.clone(),
        model: member.model.clone(),
        joined_at: member.joined_at,
        tmux_pane_id: member.tmux_pane_id.clone(),
        cwd: member.cwd.clone(),
        extra: member.extra.clone(),
    }
}

fn teams_root_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".claude").join("teams")
}

fn backup_root_from_home(home_dir: &Path, team: &str) -> Result<PathBuf, AtmError> {
    validate_path_segment(team, "team")?;
    Ok(teams_root_from_home(home_dir).join(".backups").join(team))
}

fn tasks_dir_from_home(home_dir: &Path, team: &str) -> Result<PathBuf, AtmError> {
    validate_path_segment(team, "team")?;
    Ok(home_dir.join(".claude").join("tasks").join(team))
}

fn timestamp_dir() -> String {
    let now = Utc::now();
    format!(
        "{}{:09}Z",
        now.format("%Y%m%dT%H%M%S"),
        now.timestamp_subsec_nanos()
    )
}

fn ensure_inbox_exists(inbox_path: &Path) -> Result<bool, AtmError> {
    if inbox_path.exists() {
        return Ok(false);
    }

    if let Some(parent) = inbox_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to create inbox directory {}: {error}",
                parent.display()
            ))
            .with_source(error)
            .with_recovery("Check inbox directory permissions and rerun the team recovery command.")
        })?;
    }

    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(inbox_path)
        .map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to create inbox {}: {error}",
                inbox_path.display()
            ))
            .with_source(error)
            .with_recovery("Check inbox permissions and rerun the team recovery command.")
        })?;
    Ok(true)
}

fn write_team_config(team_dir: &Path, config: &TeamConfig) -> Result<(), AtmError> {
    let config_path = team_dir.join("config.json");
    let encoded = serde_json::to_vec_pretty(config).map_err(AtmError::from)?;
    atomic_write(&config_path, &encoded)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AtmError> {
    // Test seam for deterministic rollback coverage in integration tests.
    if std::env::var_os("ATM_TEST_FAIL_TEAM_CONFIG_WRITE").is_some() {
        return Err(AtmError::file_policy(format!(
            "forced team config write failure for {}",
            path.display()
        ))
        .with_recovery(
            "Unset ATM_TEST_FAIL_TEAM_CONFIG_WRITE or rerun without the injected test failure.",
        ));
    }
    persistence::atomic_write_bytes(
        path,
        bytes,
        AtmErrorKind::FilePolicy,
        "config",
        "Check config directory permissions and rerun the operation.",
    )
}

fn copy_regular_files<F>(src: &Path, dst: &Path, include: F) -> Result<(), AtmError>
where
    F: Fn(&str) -> bool,
{
    copy_regular_files_with_policy(src, dst, include, DirEntryErrorPolicy::WarnAndSkip)
}

fn copy_regular_files_strict<F>(src: &Path, dst: &Path, include: F) -> Result<(), AtmError>
where
    F: Fn(&str) -> bool,
{
    copy_regular_files_with_policy(src, dst, include, DirEntryErrorPolicy::FailClosed)
}

enum DirEntryErrorPolicy {
    WarnAndSkip,
    FailClosed,
}

fn copy_regular_files_with_policy<F>(
    src: &Path,
    dst: &Path,
    include: F,
    dir_entry_error_policy: DirEntryErrorPolicy,
) -> Result<(), AtmError>
where
    F: Fn(&str) -> bool,
{
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to create destination directory {}: {error}",
            dst.display()
        ))
        .with_source(error)
        .with_recovery("Check destination directory permissions and retry the copy.")
    })?;

    let mut entries = Vec::new();
    for entry in fs::read_dir(src).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to read source directory {}: {error}",
            src.display()
        ))
        .with_source(error)
        .with_recovery("Check source directory permissions and retry the copy.")
    })? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => match dir_entry_error_policy {
                DirEntryErrorPolicy::WarnAndSkip => {
                    warn!(
                        source = %src.display(),
                        %error,
                        "skipping unreadable source directory entry during backup copy"
                    );
                    continue;
                }
                DirEntryErrorPolicy::FailClosed => {
                    return Err(AtmError::file_policy(format!(
                        "failed to read source directory entry under {}: {error}",
                        src.display()
                    ))
                    .with_source(error)
                    .with_recovery("Check source directory permissions and retry the restore."));
                }
            },
        };
        if entry.path().is_file() && include(&entry.file_name().to_string_lossy()) {
            entries.push(entry);
        }
    }
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        fs::copy(&from, &to).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to copy {} to {}: {error}",
                from.display(),
                to.display()
            ))
            .with_source(error)
            .with_recovery("Check source and destination permissions and retry the copy.")
        })?;
    }

    Ok(())
}

fn normalize_tmux_pane_id(pane_id: Option<&str>) -> Result<Option<String>, AtmError> {
    let Some(raw) = pane_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if let Some(rest) = raw.strip_prefix('%') {
        if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) {
            return Ok(Some(raw.to_string()));
        }
    } else if raw.chars().all(|ch| ch.is_ascii_digit()) {
        return Ok(Some(format!("%{raw}")));
    }

    Err(AtmError::validation(format!(
        "tmux pane id '{raw}' must use the tmux pane format '%<number>' or a bare numeric pane id",
    ))
    .with_recovery(
        "Pass `--pane-id $(tmux display-message -p '#{pane_id}')` or a bare numeric pane id when registering a tmux-backed member.",
    ))
}

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        AddMemberRequest, BackupRequest, RestoreRequest, add_member, backup_root_from_home,
        tasks_dir_from_home,
    };
    use crate::error_codes::AtmErrorCode;
    use crate::schema::TeamConfig;

    fn write_team_config(home_dir: &std::path::Path, team: &str) {
        let team_dir = home_dir.join(".claude").join("teams").join(team);
        std::fs::create_dir_all(&team_dir).expect("team dir");
        std::fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&TeamConfig::default()).expect("serialize config"),
        )
        .expect("write config");
    }

    #[test]
    fn add_member_rejects_invalid_member_segment() {
        let tempdir = tempdir().expect("tempdir");
        let error = AddMemberRequest::new(
            tempdir.path().to_path_buf(),
            "atm-dev",
            "../evil",
            "worker".to_string(),
            "gpt-5".to_string(),
            tempdir.path().to_path_buf(),
            None,
        )
        .expect_err("invalid member");

        assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn add_member_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");
        let error = AddMemberRequest::new(
            tempdir.path().to_path_buf(),
            "../evil",
            "arch-ctm",
            "worker".to_string(),
            "gpt-5".to_string(),
            tempdir.path().to_path_buf(),
            None,
        )
        .expect_err("invalid team");

        assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    }

    #[test]
    #[serial]
    fn add_member_normalizes_tmux_shape_when_pane_is_provided() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), "atm-dev");

        add_member(AddMemberRequest {
            home_dir: tempdir.path().to_path_buf(),
            team: "atm-dev".parse().expect("team"),
            member: "arch-ctm".parse().expect("member"),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            cwd: tempdir.path().to_path_buf(),
            tmux_pane_id: Some("7".to_string()),
        })
        .expect("add member");

        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        let config: TeamConfig = serde_json::from_slice(
            &std::fs::read(team_dir.join("config.json")).expect("read config"),
        )
        .expect("parse config");
        let member = config
            .members
            .iter()
            .find(|member| member.name == "arch-ctm")
            .expect("member");

        assert_eq!(member.tmux_pane_id, "%7");
        assert_eq!(member.extra["backendType"], serde_json::json!("tmux"));
        assert_eq!(member.extra["isActive"], serde_json::json!(true));
    }

    #[test]
    fn add_member_rejects_non_canonical_tmux_target_syntax() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), "atm-dev");

        let error = add_member(AddMemberRequest {
            home_dir: tempdir.path().to_path_buf(),
            team: "atm-dev".parse().expect("team"),
            member: "arch-ctm".parse().expect("member"),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            cwd: tempdir.path().to_path_buf(),
            tmux_pane_id: Some("session:1.2".to_string()),
        })
        .expect_err("invalid pane id");

        assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
        assert!(error.message.contains("tmux pane id"));
    }

    #[test]
    fn backup_team_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");

        let error =
            BackupRequest::new(tempdir.path().to_path_buf(), "../evil").expect_err("invalid team");

        assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn restore_team_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");

        let error = RestoreRequest::new(tempdir.path().to_path_buf(), "../evil", None, false)
            .expect_err("invalid team");

        assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn backup_root_from_home_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");
        let error = backup_root_from_home(tempdir.path(), "../evil").expect_err("invalid team");

        assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn tasks_dir_from_home_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");
        let error = tasks_dir_from_home(tempdir.path(), "../evil").expect_err("invalid team");

        assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    }
}
