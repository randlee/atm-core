use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::warn;

use crate::address::validate_path_segment;
use crate::boundary::{
    RosterHarness, RosterMemberKind, RosterMemberRecord, RosterStore, RosterStoreListTeamsRequest,
    RosterStoreLoadRosterRequest, RosterStoreReplaceRosterRequest,
};
use crate::config::{load_claude_team_config_document, load_config, resolve_team};
use crate::error::{AtmError, AtmErrorKind};
use crate::error_codes::AtmErrorCode;
use crate::home;
use crate::persistence;
use crate::roles::ROLE_TEAM_LEAD;
use crate::schema::{AgentMember, TeamConfig};
use crate::service_runtime_store;
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

/// One member entry projected from an ATM roster record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemberSummary {
    pub name: AgentName,
    pub agent_id: String,
    pub agent_type: String,
    pub model: String,
    pub joined_at: Option<u64>,
    pub tmux_pane_id: Option<String>,
    pub cwd: String,
    pub extra: serde_json::Map<String, Value>,
}

/// Result of listing all current members for one team.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    // Stronger typed/length-constrained roster metadata remains deferred to Z.11.
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
        validate_member_metadata_field("agent_type", &agent_type)?;
        validate_member_metadata_field("model", &model)?;
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

struct MemberAddContext {
    team_dir: PathBuf,
    current_extra: serde_json::Map<String, Value>,
    existing_roster: Vec<RosterMemberRecord>,
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
    // Stronger typing for backup inbox filenames remains deferred to Z.11.
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
pub fn list_teams(_home_dir: PathBuf, current_dir: PathBuf) -> Result<TeamsList, AtmError> {
    let config = load_config(&current_dir)?;
    let current_team = resolve_team(None, config.as_ref()).unwrap_or_default();
    let runtime = service_runtime_store::default_runtime()?;
    list_teams_with_roster_store(runtime.roster_store.as_ref(), current_team)
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
    let runtime = service_runtime_store::default_runtime()?;
    list_members_with_roster_store(runtime.roster_store.as_ref(), team)
}

/// Add one member record and inbox file to a team.
///
/// # Errors
///
/// Returns [`AtmError`] when the team is missing, the member already exists, or
/// inbox/config persistence fails.
pub fn add_member(request: AddMemberRequest) -> Result<AddMemberOutcome, AtmError> {
    let runtime = service_runtime_store::default_runtime()?;
    add_member_with_roster_store(runtime.roster_store.as_ref(), request)
}

fn list_teams_with_roster_store(
    roster_store: &dyn RosterStore,
    current_team: TeamName,
) -> Result<TeamsList, AtmError> {
    let mut teams = roster_store
        .list_teams(RosterStoreListTeamsRequest)
        .map(|response| response.teams)?
        .into_iter()
        .map(|team| {
            roster_store
                .load_roster(RosterStoreLoadRosterRequest { team: team.clone() })
                .map(|response| TeamSummary {
                    name: team,
                    member_count: response.members.len(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    teams.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(TeamsList {
        action: "list".to_string(),
        team: current_team,
        teams,
    })
}

fn list_members_with_roster_store(
    roster_store: &dyn RosterStore,
    team: TeamName,
) -> Result<MembersList, AtmError> {
    let roster = load_team_roster(roster_store, &team)?;
    if roster.is_empty() {
        return Err(AtmError::team_not_found(&team));
    }

    Ok(MembersList {
        team,
        members: ordered_roster_member_summaries(&roster),
    })
}

fn add_member_with_roster_store(
    roster_store: &dyn RosterStore,
    request: AddMemberRequest,
) -> Result<AddMemberOutcome, AtmError> {
    validate_member_metadata_field("agent_type", &request.agent_type)?;
    validate_member_metadata_field("model", &request.model)?;

    let MemberAddContext {
        team_dir,
        current_extra,
        mut existing_roster,
    } = load_member_add_context(roster_store, &request)?;

    let inbox_path = home::inbox_path_from_home(&request.home_dir, &request.team, &request.member)?;
    let created_inbox = ensure_inbox_exists(&inbox_path)?;
    existing_roster.push(build_member_add_roster_record(&request)?);
    replace_roster_for_member_add(roster_store, &request.team, &existing_roster)?;
    let projected_config = project_team_config_from_roster(current_extra, &existing_roster);

    if let Err(error) = write_team_config(&team_dir, &projected_config) {
        if created_inbox {
            let _ = fs::remove_file(&inbox_path);
        }
        return Err(
            error.with_recovery(
                "Check team config permissions and rerun `atm teams add-member`; ATM roster state may already include the new member.",
            )
        );
    }

    Ok(AddMemberOutcome {
        action: "add-member",
        team: request.team,
        member: request.member,
        created_inbox,
    })
}

fn load_member_add_context(
    roster_store: &dyn RosterStore,
    request: &AddMemberRequest,
) -> Result<MemberAddContext, AtmError> {
    let team_dir = home::team_dir_from_home(&request.home_dir, &request.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&request.team));
    }

    let current_extra = load_team_projection_extra_for_member_add(&team_dir)?;
    let existing_roster = load_team_roster(roster_store, &request.team)?;
    ensure_member_absent(&existing_roster, &request.team, &request.member)?;
    Ok(MemberAddContext {
        team_dir,
        current_extra,
        existing_roster,
    })
}

fn ensure_member_absent(
    existing_roster: &[RosterMemberRecord],
    team: &TeamName,
    member: &AgentName,
) -> Result<(), AtmError> {
    if existing_roster
        .iter()
        .any(|existing_member| existing_member.agent_name == *member)
    {
        return Err(AtmError::new_with_code(
            AtmErrorCode::IdentityConflict,
            AtmErrorKind::Validation,
            format!("member '{}' already exists in team '{}'", member, team),
        )
        .with_recovery(
            "Use `atm members` to inspect the current ATM roster and choose a new member name before retrying `atm team member add`.",
        ));
    }
    Ok(())
}

fn build_member_add_roster_record(
    request: &AddMemberRequest,
) -> Result<RosterMemberRecord, AtmError> {
    let normalized_tmux_pane_id = normalize_tmux_pane_id(request.tmux_pane_id.as_deref())?;
    let mut extra = serde_json::Map::new();
    if normalized_tmux_pane_id.is_some() {
        extra.insert("backendType".to_string(), json!("tmux"));
        extra.insert("isActive".to_string(), json!(true));
    }
    extra.insert(
        "agentId".to_string(),
        json!(format!("{}@{}", request.member, request.team)),
    );
    extra.insert(
        "joinedAt".to_string(),
        json!(Utc::now().timestamp_millis() as u64),
    );
    extra.insert("cwd".to_string(), json!(request.cwd.display().to_string()));

    Ok(RosterMemberRecord {
        team_name: request.team.clone(),
        agent_name: request.member.clone(),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::ClaudeCode,
        agent_type: request.agent_type.clone(),
        model: request.model.clone(),
        recipient_pane_id: normalized_tmux_pane_id,
        metadata_json: extra,
    })
}

fn replace_roster_for_member_add(
    roster_store: &dyn RosterStore,
    team: &TeamName,
    existing_roster: &[RosterMemberRecord],
) -> Result<(), AtmError> {
    roster_store
        .replace_roster(RosterStoreReplaceRosterRequest {
            team: team.clone(),
            members: existing_roster.to_vec(),
            source: None,
        })
        .map(|_| ())
        .map_err(|error| {
            error.with_recovery(
                "Check ATM roster store availability and rerun `atm teams add-member`.",
            )
        })
}

/// Create a point-in-time backup of one team's config, inboxes, and task files.
///
/// # Errors
///
/// Returns [`AtmError`] when the team/config is missing or backup directory/file
/// creation fails.
pub fn backup_team(request: BackupRequest) -> Result<BackupOutcome, AtmError> {
    let runtime = service_runtime_store::default_runtime()?;
    backup_team_with_roster_store(runtime.roster_store.as_ref(), request)
}

fn backup_team_with_roster_store(
    roster_store: &dyn RosterStore,
    request: BackupRequest,
) -> Result<BackupOutcome, AtmError> {
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
    write_roster_audit_snapshot(&backup_dir, roster_store, &request.team)?;

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
    let runtime = service_runtime_store::default_runtime()?;
    restore::restore_team_with_roster_store(runtime.roster_store.as_ref(), request)
}

fn ordered_roster_member_summaries(records: &[RosterMemberRecord]) -> Vec<MemberSummary> {
    let mut members = Vec::with_capacity(records.len());
    if let Some(team_lead) = records
        .iter()
        .find(|member| member.agent_name == ROLE_TEAM_LEAD)
    {
        members.push(member_summary_from_roster(team_lead));
    }
    for member in records {
        if member.agent_name == ROLE_TEAM_LEAD {
            continue;
        }
        members.push(member_summary_from_roster(member));
    }
    members
}

fn member_summary_from_roster(record: &RosterMemberRecord) -> MemberSummary {
    MemberSummary {
        name: record.agent_name.clone(),
        agent_id: metadata_string(&record.metadata_json, "agentId")
            .unwrap_or_else(|| format!("{}@{}", record.agent_name, record.team_name)),
        agent_type: record.agent_type.clone(),
        model: record.model.clone(),
        joined_at: metadata_u64(&record.metadata_json, "joinedAt"),
        tmux_pane_id: record.recipient_pane_id.clone(),
        cwd: metadata_string(&record.metadata_json, "cwd").unwrap_or_default(),
        extra: compatibility_extra_fields(&record.metadata_json),
    }
}

const MAX_MEMBER_METADATA_FIELD_LEN: usize = 256;

fn validate_member_metadata_field(field: &str, value: &str) -> Result<(), AtmError> {
    if value.len() > MAX_MEMBER_METADATA_FIELD_LEN {
        return Err(AtmError::validation(format!(
            "{field} must be at most {MAX_MEMBER_METADATA_FIELD_LEN} bytes"
        )));
    }
    Ok(())
}

fn load_team_projection_extra_for_member_add(
    team_dir: &Path,
) -> Result<serde_json::Map<String, Value>, AtmError> {
    // Z.11 still preserves non-roster Claude config extras while add-member
    // projects canonical ATM roster truth back into config.json.
    load_claude_team_config_document(team_dir).map(|config| config.extra)
}

fn load_team_roster(
    roster_store: &dyn RosterStore,
    team: &TeamName,
) -> Result<Vec<RosterMemberRecord>, AtmError> {
    roster_store
        .load_roster(RosterStoreLoadRosterRequest { team: team.clone() })
        .map(|response| response.members)
}

pub(super) fn project_team_config_from_roster(
    extra: serde_json::Map<String, Value>,
    records: &[RosterMemberRecord],
) -> TeamConfig {
    let mut members = Vec::with_capacity(records.len());
    if let Some(team_lead) = records
        .iter()
        .find(|member| member.agent_name == ROLE_TEAM_LEAD)
    {
        members.push(agent_member_from_roster_record(team_lead));
    }
    for record in records {
        if record.agent_name == ROLE_TEAM_LEAD {
            continue;
        }
        members.push(agent_member_from_roster_record(record));
    }
    TeamConfig { members, extra }
}

fn agent_member_from_roster_record(record: &RosterMemberRecord) -> AgentMember {
    let mut extra = compatibility_extra_fields(&record.metadata_json);
    AgentMember {
        name: record.agent_name.clone(),
        agent_id: metadata_string(&record.metadata_json, "agentId")
            .unwrap_or_else(|| format!("{}@{}", record.agent_name, record.team_name)),
        agent_type: record.agent_type.clone().into(),
        model: record.model.clone(),
        joined_at: metadata_u64(&record.metadata_json, "joinedAt"),
        tmux_pane_id: record.recipient_pane_id.clone(),
        cwd: metadata_string(&record.metadata_json, "cwd").unwrap_or_default(),
        extra: {
            extra.remove("agentId");
            extra.remove("joinedAt");
            extra.remove("cwd");
            extra
        },
    }
}

fn compatibility_extra_fields(
    metadata_json: &serde_json::Map<String, Value>,
) -> serde_json::Map<String, Value> {
    let mut extra = metadata_json.clone();
    extra.remove("agentId");
    extra.remove("joinedAt");
    extra.remove("cwd");
    extra
}

fn metadata_string(metadata_json: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    metadata_json
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn metadata_u64(metadata_json: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    metadata_json.get(key).and_then(Value::as_u64)
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

fn write_roster_audit_snapshot(
    backup_dir: &Path,
    roster_store: &dyn RosterStore,
    team: &TeamName,
) -> Result<(), AtmError> {
    let roster = load_team_roster(roster_store, team)?;
    let bytes = serde_json::to_vec_pretty(&json!({
        "team": team,
        "members": roster,
    }))
    .map_err(AtmError::from)?;
    persistence::atomic_write_bytes(
        &backup_dir.join("atm-roster.json"),
        &bytes,
        AtmErrorKind::FilePolicy,
        "ATM roster backup snapshot",
        "Check backup directory permissions and retry the backup.",
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
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        AddMemberRequest, BackupRequest, MAX_MEMBER_METADATA_FIELD_LEN, RestoreRequest,
        add_member_with_roster_store, backup_root_from_home, backup_team_with_roster_store,
        list_members_with_roster_store, list_teams_with_roster_store, tasks_dir_from_home,
    };
    use crate::boundary::{
        self, RosterHarness, RosterMemberKind, RosterMemberRecord, RosterStore,
        RosterStoreHealthSnapshot, RosterStoreHealthSnapshotRequest,
        RosterStoreHealthSnapshotResponse, RosterStoreListTeamsRequest,
        RosterStoreListTeamsResponse, RosterStoreLoadRosterRequest, RosterStoreLoadRosterResponse,
        RosterStoreQueryMembershipRequest, RosterStoreQueryMembershipResponse,
        RosterStoreReplaceRosterRequest, RosterStoreReplaceRosterResponse,
    };
    use crate::error_codes::AtmErrorCode;
    use crate::schema::TeamConfig;
    use crate::test_support::{ROLE_TEAM_LEAD, TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use crate::types::TeamName;

    #[derive(Default)]
    struct RecordingRosterStore {
        // Test-only seam: Mutex keeps the fixture simple while serial tests own all access.
        teams: Mutex<BTreeMap<TeamName, Vec<RosterMemberRecord>>>,
    }

    impl RecordingRosterStore {
        fn seed_team(&self, team: &str, members: Vec<RosterMemberRecord>) {
            self.teams
                .lock()
                .expect("roster store lock")
                .insert(team.parse().expect("team"), members);
        }
    }

    impl boundary::sealed::Sealed for RecordingRosterStore {}

    impl RosterStore for RecordingRosterStore {
        fn replace_roster(
            &self,
            request: RosterStoreReplaceRosterRequest,
        ) -> Result<RosterStoreReplaceRosterResponse, crate::error::AtmError> {
            self.teams
                .lock()
                .expect("roster store lock")
                .insert(request.team.clone(), request.members.clone());
            let current_member_count = request.members.len() as u64;
            Ok(RosterStoreReplaceRosterResponse {
                team: request.team,
                previous_member_count: 0,
                current_member_count,
                replaced: true,
            })
        }

        fn load_roster(
            &self,
            request: RosterStoreLoadRosterRequest,
        ) -> Result<RosterStoreLoadRosterResponse, crate::error::AtmError> {
            let members = self
                .teams
                .lock()
                .expect("roster store lock")
                .get(&request.team)
                .cloned()
                .unwrap_or_default();
            Ok(RosterStoreLoadRosterResponse {
                team: request.team,
                members,
            })
        }

        fn query_membership(
            &self,
            request: RosterStoreQueryMembershipRequest,
        ) -> Result<RosterStoreQueryMembershipResponse, crate::error::AtmError> {
            let member = self
                .teams
                .lock()
                .expect("roster store lock")
                .get(&request.team)
                .and_then(|members| {
                    members
                        .iter()
                        .find(|member| member.agent_name == request.member)
                        .cloned()
                });
            Ok(RosterStoreQueryMembershipResponse {
                team: request.team,
                is_member: member.is_some(),
                member,
            })
        }

        fn list_teams(
            &self,
            _request: RosterStoreListTeamsRequest,
        ) -> Result<RosterStoreListTeamsResponse, crate::error::AtmError> {
            let teams = self
                .teams
                .lock()
                .expect("roster store lock")
                .keys()
                .cloned()
                .collect();
            Ok(RosterStoreListTeamsResponse { teams })
        }

        fn health_snapshot(
            &self,
            request: RosterStoreHealthSnapshotRequest,
        ) -> Result<RosterStoreHealthSnapshotResponse, crate::error::AtmError> {
            let member_count = self
                .teams
                .lock()
                .expect("roster store lock")
                .get(&request.team)
                .map(|members| members.len() as u64)
                .unwrap_or_default();
            Ok(RosterStoreHealthSnapshotResponse {
                snapshot: RosterStoreHealthSnapshot {
                    team: request.team,
                    member_count,
                    stale: false,
                    refreshed_at: None,
                },
            })
        }
    }

    fn roster_member(team: &str, agent: &str) -> RosterMemberRecord {
        RosterMemberRecord {
            team_name: team.parse().expect("team"),
            agent_name: agent.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            recipient_pane_id: None,
            metadata_json: serde_json::Map::new(),
        }
    }

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
            TEST_TEAM,
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
            TEST_SENDER,
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
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();

        add_member_with_roster_store(
            &roster_store,
            AddMemberRequest {
                home_dir: tempdir.path().to_path_buf(),
                team: TEST_TEAM.parse().expect("team"),
                member: TEST_SENDER.parse().expect("member"),
                agent_type: "worker".to_string(),
                model: "gpt-5".to_string(),
                cwd: tempdir.path().to_path_buf(),
                tmux_pane_id: Some("7".to_string()),
            },
        )
        .expect("add member");

        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let config: TeamConfig = serde_json::from_slice(
            &std::fs::read(team_dir.join("config.json")).expect("read config"),
        )
        .expect("parse config");
        let member = config
            .members
            .iter()
            .find(|member| member.name == TEST_SENDER)
            .expect("member");

        assert_eq!(member.tmux_pane_id.as_deref(), Some("%7"));
        assert_eq!(member.extra["backendType"], serde_json::json!("tmux"));
        assert_eq!(member.extra["isActive"], serde_json::json!(true));

        let roster = roster_store
            .load_roster(RosterStoreLoadRosterRequest {
                team: TEST_TEAM.parse().expect("team"),
            })
            .expect("load roster")
            .members;
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].recipient_pane_id.as_deref(), Some("%7"));
    }

    #[test]
    fn add_member_rejects_non_canonical_tmux_target_syntax() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();

        let error = add_member_with_roster_store(
            &roster_store,
            AddMemberRequest {
                home_dir: tempdir.path().to_path_buf(),
                team: TEST_TEAM.parse().expect("team"),
                member: TEST_SENDER.parse().expect("member"),
                agent_type: "worker".to_string(),
                model: "gpt-5".to_string(),
                cwd: tempdir.path().to_path_buf(),
                tmux_pane_id: Some("session:1.2".to_string()),
            },
        )
        .expect_err("invalid pane id");

        assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
        assert!(error.message.contains("tmux pane id"));
    }

    #[test]
    fn list_members_reports_atm_roster_truth_without_file_members() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        let mut member = roster_member(TEST_TEAM, TEST_SENDER);
        member.recipient_pane_id = Some("%9".to_string());
        member
            .metadata_json
            .insert("cwd".to_string(), serde_json::json!("/tmp/worker"));
        roster_store.seed_team(TEST_TEAM, vec![member]);

        let members =
            list_members_with_roster_store(&roster_store, TEST_TEAM.parse().expect("team"))
                .expect("list members");

        assert_eq!(members.team.as_str(), TEST_TEAM);
        assert_eq!(members.members.len(), 1);
        assert_eq!(members.members[0].name.as_str(), TEST_SENDER);
        assert_eq!(members.members[0].tmux_pane_id.as_deref(), Some("%9"));
        assert_eq!(members.members[0].cwd, "/tmp/worker");
    }

    #[test]
    fn list_teams_reports_atm_roster_truth() {
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(TEST_TEAM, vec![roster_member(TEST_TEAM, TEST_SENDER)]);
        roster_store.seed_team(
            "other-team",
            vec![
                roster_member("other-team", TEST_SENDER),
                roster_member("other-team", TEST_RECIPIENT),
            ],
        );

        let teams = list_teams_with_roster_store(&roster_store, TEST_TEAM.parse().expect("team"))
            .expect("list teams");

        assert_eq!(teams.team.as_str(), TEST_TEAM);
        assert_eq!(teams.teams.len(), 2);
        assert_eq!(teams.teams[0].name.as_str(), "other-team");
        assert_eq!(teams.teams[0].member_count, 2);
        assert_eq!(teams.teams[1].name.as_str(), TEST_TEAM);
        assert_eq!(teams.teams[1].member_count, 1);
    }

    #[test]
    #[serial]
    fn add_member_projects_config_from_updated_atm_roster_truth() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        let mut existing = roster_member(TEST_TEAM, ROLE_TEAM_LEAD);
        existing.agent_type = "lead".to_string();
        existing.model = "gpt-5".to_string();
        existing
            .metadata_json
            .insert("cwd".to_string(), serde_json::json!("/tmp/team-lead"));
        roster_store.seed_team(TEST_TEAM, vec![existing]);

        add_member_with_roster_store(
            &roster_store,
            AddMemberRequest {
                home_dir: tempdir.path().to_path_buf(),
                team: TEST_TEAM.parse().expect("team"),
                member: TEST_SENDER.parse().expect("member"),
                agent_type: "worker".to_string(),
                model: "gpt-5".to_string(),
                cwd: tempdir.path().to_path_buf(),
                tmux_pane_id: Some("%12".to_string()),
            },
        )
        .expect("add member");

        let roster = roster_store
            .load_roster(RosterStoreLoadRosterRequest {
                team: TEST_TEAM.parse().expect("team"),
            })
            .expect("load roster")
            .members;
        assert_eq!(roster.len(), 2);
        assert!(roster.iter().any(|member| member.agent_name == TEST_SENDER));

        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let config: TeamConfig = serde_json::from_slice(
            &std::fs::read(team_dir.join("config.json")).expect("read config"),
        )
        .expect("parse config");
        assert_eq!(config.members.len(), 2);
        let member = config
            .members
            .iter()
            .find(|member| member.name == TEST_SENDER)
            .expect("member");
        assert_eq!(member.tmux_pane_id.as_deref(), Some("%12"));
    }

    #[test]
    fn backup_team_writes_atm_roster_audit_snapshot() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(
            TEST_TEAM,
            vec![
                roster_member(TEST_TEAM, ROLE_TEAM_LEAD),
                roster_member(TEST_TEAM, TEST_SENDER),
            ],
        );

        let outcome = backup_team_with_roster_store(
            &roster_store,
            BackupRequest::new(tempdir.path().to_path_buf(), TEST_TEAM).expect("request"),
        )
        .expect("backup");

        let snapshot: serde_json::Value = serde_json::from_slice(
            &std::fs::read(outcome.backup_path.join("atm-roster.json")).expect("snapshot"),
        )
        .expect("parse snapshot");
        assert_eq!(snapshot["team"], serde_json::json!(TEST_TEAM));
        assert_eq!(snapshot["members"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn add_member_rejects_overlong_model_metadata() {
        let tempdir = tempdir().expect("tempdir");
        let error = AddMemberRequest::new(
            tempdir.path().to_path_buf(),
            TEST_TEAM,
            TEST_SENDER,
            "worker".to_string(),
            "m".repeat(MAX_MEMBER_METADATA_FIELD_LEN + 1),
            tempdir.path().to_path_buf(),
            None,
        )
        .expect_err("invalid model");

        assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
        assert!(error.message.contains("model"));
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
