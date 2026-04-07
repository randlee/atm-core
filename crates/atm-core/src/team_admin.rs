use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use tracing::warn;

use crate::config::{load_config, load_team_config, resolve_team};
use crate::error::AtmError;
use crate::home;
use crate::schema::{AgentMember, TeamConfig};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TeamSummary {
    pub name: String,
    pub member_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TeamsList {
    pub action: &'static str,
    pub teams: Vec<TeamSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MemberSummary {
    pub name: String,
    pub agent_id: String,
    pub agent_type: String,
    pub model: String,
    pub joined_at: Option<u64>,
    pub tmux_pane_id: String,
    pub cwd: String,
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MembersList {
    pub team: String,
    pub members: Vec<MemberSummary>,
}

#[derive(Debug, Clone)]
pub struct MembersQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub team_override: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AddMemberRequest {
    pub home_dir: PathBuf,
    pub team: String,
    pub member: String,
    pub agent_type: String,
    pub model: String,
    pub cwd: PathBuf,
    pub tmux_pane_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AddMemberOutcome {
    pub action: &'static str,
    pub team: String,
    pub member: String,
    pub created_inbox: bool,
}

#[derive(Debug, Clone)]
pub struct BackupRequest {
    pub home_dir: PathBuf,
    pub team: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BackupOutcome {
    pub action: &'static str,
    pub team: String,
    pub backup_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RestoreRequest {
    pub home_dir: PathBuf,
    pub team: String,
    pub from: Option<PathBuf>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RestorePlan {
    pub action: &'static str,
    pub team: String,
    pub backup_path: PathBuf,
    pub dry_run: bool,
    pub would_restore_members: Vec<String>,
    pub would_restore_inboxes: Vec<String>,
    pub would_restore_tasks: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub action: &'static str,
    pub team: String,
    pub backup_path: PathBuf,
    pub members_restored: usize,
    pub inboxes_restored: usize,
    pub tasks_restored: usize,
}

pub fn list_teams(home_dir: PathBuf) -> Result<TeamsList, AtmError> {
    let teams_root = teams_root_from_home(&home_dir);
    if !teams_root.exists() {
        return Ok(TeamsList {
            action: "list",
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
    })? {
        let entry = entry.map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read teams directory entry under {}: {error}",
                teams_root.display()
            ))
            .with_source(error)
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
                name: entry.file_name().to_string_lossy().to_string(),
                member_count: config.members.len(),
            }),
            Err(error) => warn!(
                path = %path.display(),
                %error,
                "skipping malformed team config while listing teams"
            ),
        }
    }

    teams.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(TeamsList {
        action: "list",
        teams,
    })
}

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

pub fn add_member(request: AddMemberRequest) -> Result<AddMemberOutcome, AtmError> {
    let team_dir = home::team_dir_from_home(&request.home_dir, &request.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&request.team));
    }

    let mut config = load_team_config(&team_dir)?;
    if config
        .members
        .iter()
        .any(|member| member.name == request.member)
    {
        return Err(AtmError::validation(format!(
            "member '{}' already exists in team '{}'",
            request.member, request.team
        )));
    }

    let inbox_path = home::inbox_path_from_home(&request.home_dir, &request.team, &request.member)?;
    let created_inbox = ensure_inbox_exists(&inbox_path)?;

    config.members.push(AgentMember {
        name: request.member.clone(),
        agent_id: format!("{}@{}", request.member, request.team),
        agent_type: request.agent_type,
        model: request.model,
        joined_at: Some(Utc::now().timestamp_millis() as u64),
        tmux_pane_id: request.tmux_pane_id.unwrap_or_default(),
        cwd: request.cwd.display().to_string(),
        extra: serde_json::Map::new(),
    });

    if let Err(error) = write_team_config(&team_dir, &config) {
        if created_inbox {
            let _ = fs::remove_file(&inbox_path);
        }
        return Err(error);
    }

    Ok(AddMemberOutcome {
        action: "add-member",
        team: request.team,
        member: request.member,
        created_inbox,
    })
}

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

    let backup_dir = backup_root_from_home(&request.home_dir, &request.team).join(timestamp_dir());
    fs::create_dir_all(backup_dir.join("inboxes")).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to create backup directory {}: {error}",
            backup_dir.display()
        ))
        .with_source(error)
    })?;

    fs::copy(&config_path, backup_dir.join("config.json")).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to copy {} into backup {}: {error}",
            config_path.display(),
            backup_dir.display()
        ))
        .with_source(error)
    })?;

    copy_regular_files(&team_dir.join("inboxes"), &backup_dir.join("inboxes"))?;
    copy_regular_files(
        &tasks_dir_from_home(&request.home_dir, &request.team),
        &backup_dir.join("tasks"),
    )?;

    Ok(BackupOutcome {
        action: "backup",
        team: request.team,
        backup_path: backup_dir,
    })
}

pub fn restore_team(
    request: RestoreRequest,
) -> Result<Result<RestoreOutcome, RestorePlan>, AtmError> {
    let team_dir = home::team_dir_from_home(&request.home_dir, &request.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&request.team));
    }
    let current_config = load_team_config(&team_dir)?;
    let backup_dir = locate_backup_dir(&request.home_dir, &request.team, request.from.as_deref())?;
    let backup_config = load_team_config(&backup_dir)?;

    let members_to_restore = backup_config
        .members
        .iter()
        .filter(|member| member.name != "team-lead")
        .filter(|member| {
            !current_config
                .members
                .iter()
                .any(|existing| existing.name == member.name)
        })
        .map(|member| member.name.clone())
        .collect::<Vec<_>>();

    let mut inboxes_to_restore = list_backup_inboxes(&backup_dir)?;
    inboxes_to_restore.retain(|name| name != "team-lead.json");
    let tasks_to_restore = count_numeric_task_files(&backup_dir.join("tasks"))?;

    if request.dry_run {
        return Ok(Err(RestorePlan {
            action: "restore",
            team: request.team,
            backup_path: backup_dir,
            dry_run: true,
            would_restore_members: members_to_restore,
            would_restore_inboxes: inboxes_to_restore,
            would_restore_tasks: tasks_to_restore,
        }));
    }

    let mut updated_config = current_config.clone();
    for member in &backup_config.members {
        if member.name == "team-lead" {
            continue;
        }
        if updated_config
            .members
            .iter()
            .any(|existing| existing.name == member.name)
        {
            continue;
        }
        let mut restored = member.clone();
        clear_runtime_member_state(&mut restored);
        updated_config.members.push(restored);
    }
    if let Some(value) = current_config.extra.get("leadSessionId") {
        updated_config
            .extra
            .insert("leadSessionId".to_string(), value.clone());
    }

    let inboxes_dir = team_dir.join("inboxes");
    fs::create_dir_all(&inboxes_dir).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create inbox directory {}: {error}",
            inboxes_dir.display()
        ))
        .with_source(error)
    })?;
    for inbox_name in &inboxes_to_restore {
        let from = backup_dir.join("inboxes").join(inbox_name);
        let to = inboxes_dir.join(inbox_name);
        fs::copy(&from, &to).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to restore inbox {} from {}: {error}",
                to.display(),
                from.display()
            ))
            .with_source(error)
        })?;
    }

    let tasks_dir = tasks_dir_from_home(&request.home_dir, &request.team);
    restore_task_bucket(&backup_dir.join("tasks"), &tasks_dir)?;
    recompute_highwatermark(&tasks_dir)?;
    write_team_config(&team_dir, &updated_config)?;

    Ok(Ok(RestoreOutcome {
        action: "restore",
        team: request.team,
        backup_path: backup_dir,
        members_restored: members_to_restore.len(),
        inboxes_restored: inboxes_to_restore.len(),
        tasks_restored: tasks_to_restore,
    }))
}

fn member_summary(member: &AgentMember) -> MemberSummary {
    MemberSummary {
        name: member.name.clone(),
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

fn backup_root_from_home(home_dir: &Path, team: &str) -> PathBuf {
    teams_root_from_home(home_dir).join(".backups").join(team)
}

fn tasks_dir_from_home(home_dir: &Path, team: &str) -> PathBuf {
    home_dir.join(".claude").join("tasks").join(team)
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
        })?;
    Ok(true)
}

fn write_team_config(team_dir: &Path, config: &TeamConfig) -> Result<(), AtmError> {
    let config_path = team_dir.join("config.json");
    let encoded = serde_json::to_vec_pretty(config).map_err(AtmError::from)?;
    atomic_write(&config_path, &encoded)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AtmError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to create parent directory {}: {error}",
                parent.display()
            ))
            .with_source(error)
        })?;
    }

    // Test seam for deterministic rollback coverage in integration tests.
    if std::env::var_os("ATM_TEST_FAIL_TEAM_CONFIG_WRITE").is_some() {
        return Err(AtmError::file_policy(format!(
            "forced team config write failure for {}",
            path.display()
        )));
    }

    let temp_path = path.with_file_name(format!(
        ".{}.tmp.{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config"),
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::write(&temp_path, bytes).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to write temporary file {}: {error}",
            temp_path.display()
        ))
        .with_source(error)
    })?;
    fs::rename(&temp_path, path).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to atomically replace {}: {error}",
            path.display()
        ))
        .with_source(error)
    })?;
    Ok(())
}

fn copy_regular_files(src: &Path, dst: &Path) -> Result<(), AtmError> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to create destination directory {}: {error}",
            dst.display()
        ))
        .with_source(error)
    })?;

    let mut entries = fs::read_dir(src)
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read source directory {}: {error}",
                src.display()
            ))
            .with_source(error)
        })?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .collect::<Vec<_>>();
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
        })?;
    }

    Ok(())
}

fn locate_backup_dir(
    home_dir: &Path,
    team: &str,
    explicit: Option<&Path>,
) -> Result<PathBuf, AtmError> {
    if let Some(path) = explicit {
        if !path.is_dir() {
            return Err(AtmError::missing_document(format!(
                "backup directory not found: {}",
                path.display()
            )));
        }
        return Ok(path.to_path_buf());
    }

    let root = backup_root_from_home(home_dir, team);
    if !root.exists() {
        return Err(AtmError::missing_document(format!(
            "no backup found for team '{}'",
            team
        )));
    }
    let mut entries = fs::read_dir(&root)
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read backup directory {}: {error}",
                root.display()
            ))
            .with_source(error)
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    entries.sort();
    entries
        .pop()
        .ok_or_else(|| AtmError::missing_document(format!("no backup found for team '{}'", team)))
}

fn list_backup_inboxes(backup_dir: &Path) -> Result<Vec<String>, AtmError> {
    let inbox_dir = backup_dir.join("inboxes");
    if !inbox_dir.exists() {
        return Ok(Vec::new());
    }

    let mut names = fs::read_dir(&inbox_dir)
        .map_err(|error| {
            AtmError::mailbox_read(format!(
                "failed to read backup inbox directory {}: {error}",
                inbox_dir.display()
            ))
            .with_source(error)
        })?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

fn count_numeric_task_files(tasks_dir: &Path) -> Result<usize, AtmError> {
    if !tasks_dir.exists() {
        return Ok(0);
    }
    let count = fs::read_dir(tasks_dir)
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read task directory {}: {error}",
                tasks_dir.display()
            ))
            .with_source(error)
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some("json")
                && path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .and_then(|stem| stem.parse::<u64>().ok())
                    .is_some()
        })
        .count();
    Ok(count)
}

fn restore_task_bucket(src: &Path, dst: &Path) -> Result<(), AtmError> {
    if !src.exists() {
        fs::create_dir_all(dst).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to create task directory {}: {error}",
                dst.display()
            ))
            .with_source(error)
        })?;
        return Ok(());
    }

    let staging = dst.with_file_name(format!(
        ".{}.restore.{}",
        dst.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tasks"),
        std::process::id()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to clear task staging directory {}: {error}",
                staging.display()
            ))
            .with_source(error)
        })?;
    }
    copy_regular_files(src, &staging)?;

    if dst.exists() {
        fs::remove_dir_all(dst).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to clear existing task directory {}: {error}",
                dst.display()
            ))
            .with_source(error)
        })?;
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to create task parent directory {}: {error}",
                parent.display()
            ))
            .with_source(error)
        })?;
    }
    fs::rename(&staging, dst).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to install restored task directory {}: {error}",
            dst.display()
        ))
        .with_source(error)
    })?;
    Ok(())
}

fn recompute_highwatermark(tasks_dir: &Path) -> Result<usize, AtmError> {
    fs::create_dir_all(tasks_dir).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to create task directory {}: {error}",
            tasks_dir.display()
        ))
        .with_source(error)
    })?;

    let max_id = fs::read_dir(tasks_dir)
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read task directory {}: {error}",
                tasks_dir.display()
            ))
            .with_source(error)
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json")
        })
        .filter_map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<usize>().ok())
        })
        .max()
        .unwrap_or(0);

    fs::write(tasks_dir.join(".highwatermark"), format!("{max_id}\n")).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to write {}: {error}",
            tasks_dir.join(".highwatermark").display()
        ))
        .with_source(error)
    })?;
    Ok(max_id)
}

fn clear_runtime_member_state(member: &mut AgentMember) {
    member.tmux_pane_id.clear();
    for key in [
        "sessionId",
        "activity",
        "status",
        "lastAliveAt",
        "processId",
        "isActive",
        "lastActive",
        "paneId",
    ] {
        member.extra.remove(key);
    }
}
