#![allow(
    deprecated,
    reason = "team_admin still uses the legacy atm-core roster boundary until the retained admin flows finish migrating to canonical shared storage seams"
)]

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::boundary::{RosterEntry, RosterHarness, RosterMemberKind, RosterStore};
use crate::config::load_claude_team_config_document;
use crate::error::AtmError;
use crate::home;
use crate::schema::{AgentType, HOME_DIR_METADATA_KEY, HomeDirPath};
use crate::types::{AgentName, ModelName, PaneId, TeamName};

#[path = "team_admin/filesystem.rs"]
mod filesystem;
#[path = "team_admin/projection.rs"]
mod projection;
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
    pub model: ModelName,
    pub joined_at: Option<u64>,
    pub tmux_pane_id: Option<PaneId>,
    pub home_dir: HomeDirPath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_cwd: Option<String>,
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
    pub team: TeamName,
    pub caller_identity: Option<AgentName>,
    pub live_cwd: Option<PathBuf>,
}

/// Semantic target member for roster repair operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberName(pub AgentName);

/// Parameters for adding one member to a team roster.
#[derive(Debug, Clone)]
pub struct AddMemberRequest {
    pub atm_home_dir: HomeDirPath,
    pub team: TeamName,
    pub member: AgentName,
    pub agent_type: AgentType,
    pub model: ModelName,
    pub member_home_dir: HomeDirPath,
    pub tmux_pane_id: Option<PaneId>,
}

impl AddMemberRequest {
    pub fn new(
        atm_home_dir: PathBuf,
        team: &str,
        member: &str,
        agent_type: String,
        model: String,
        member_home_dir: PathBuf,
        tmux_pane_id: Option<String>,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            atm_home_dir: atm_home_dir.into(),
            team: team.parse()?,
            member: member.parse()?,
            agent_type: parse_agent_type(agent_type)?,
            model: ModelName::new(model)?,
            member_home_dir: member_home_dir.into(),
            tmux_pane_id: normalize_tmux_pane_id(tmux_pane_id.as_deref())?,
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

/// Parameters for updating one existing team member metadata row.
#[derive(Debug, Clone)]
pub struct UpdateMemberRequest {
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub team: TeamName,
    pub member: MemberName,
    pub home_dir: Option<HomeDirPath>,
    pub harness: Option<RosterHarness>,
    pub agent_type: Option<AgentType>,
    pub model: Option<ModelName>,
    pub tmux_pane_id: Option<PaneId>,
}

impl UpdateMemberRequest {
    #[allow(
        clippy::too_many_arguments,
        reason = "CLI-owned repair path constructs one request from flat flags"
    )]
    pub fn new(
        caller_identity: AgentName,
        caller_team: TeamName,
        team: &str,
        member: &str,
        home_dir: Option<PathBuf>,
        harness: Option<String>,
        agent_type: Option<String>,
        model: Option<String>,
        tmux_pane_id: Option<String>,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            caller_identity,
            caller_team,
            team: team.parse()?,
            member: MemberName(member.parse()?),
            home_dir: home_dir.map(Into::into),
            harness: harness.map(parse_roster_harness).transpose()?,
            agent_type: agent_type.map(parse_agent_type).transpose()?,
            model: model.map(ModelName::new).transpose()?,
            tmux_pane_id: normalize_tmux_pane_id(tmux_pane_id.as_deref())?,
        })
    }
}

/// Result of updating one existing member metadata row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateMemberOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub member: AgentName,
}

struct MemberAddContext {
    team_dir: PathBuf,
    current_extra: serde_json::Map<String, Value>,
    existing_roster: Vec<RosterEntry>,
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
    // Dry-run output intentionally preserves raw backup inbox filenames so the
    // operator sees the exact on-disk artifact names that would be replayed.
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
pub fn list_teams_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    current_team: TeamName,
) -> Result<TeamsList, AtmError> {
    list_teams_from_roster_store(roster_store, current_team)
}

/// List the current member roster for one team.
///
/// # Errors
///
/// Returns [`AtmError`] when team resolution fails, the team directory is
/// missing, or `config.json` cannot be loaded.
pub fn list_members_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    query: MembersQuery,
) -> Result<MembersList, AtmError> {
    list_members_from_roster_store(roster_store, query)
}

/// Add one member record and inbox file to a team.
///
/// # Errors
///
/// Returns [`AtmError`] when the team is missing, the member already exists, or
/// inbox/config persistence fails.
pub fn add_member_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    request: AddMemberRequest,
) -> Result<AddMemberOutcome, AtmError> {
    add_member_from_roster_store(roster_store, request)
}

/// Update one existing member record through the retained local repair path.
///
/// # Errors
///
/// Returns [`AtmError`] when the team is missing, the member does not exist,
/// or roster/config persistence fails.
pub fn update_member_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    atm_home_dir: &Path,
    request: UpdateMemberRequest,
) -> Result<UpdateMemberOutcome, AtmError> {
    update_member_from_roster_store(roster_store, atm_home_dir, request)
}

fn list_teams_from_roster_store(
    roster_store: &dyn RosterStore,
    current_team: TeamName,
) -> Result<TeamsList, AtmError> {
    let mut teams = roster_store
        .list_teams()?
        .into_iter()
        .map(|team| {
            roster_store.load_roster(&team).map(|members| TeamSummary {
                name: team,
                member_count: members.len(),
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

fn list_members_from_roster_store(
    roster_store: &dyn RosterStore,
    query: MembersQuery,
) -> Result<MembersList, AtmError> {
    projection::list_members_from_roster_store(roster_store, query)
}

fn add_member_from_roster_store(
    roster_store: &dyn RosterStore,
    request: AddMemberRequest,
) -> Result<AddMemberOutcome, AtmError> {
    let MemberAddContext {
        team_dir,
        current_extra,
        mut existing_roster,
    } = load_member_add_context(roster_store, &request)?;

    let inbox_path = home::inbox_path_from_home(
        request.atm_home_dir.as_ref(),
        &request.team,
        &request.member,
    )?;
    let created_inbox = filesystem::ensure_inbox_exists(&inbox_path)?;
    existing_roster.push(build_member_add_roster_record(&request));
    replace_roster_for_member_add(roster_store, &request.team, &existing_roster)?;
    let projected_config =
        projection::project_team_config_from_roster(current_extra, &existing_roster)?;

    if let Err(error) = filesystem::write_team_config(&team_dir, &projected_config) {
        if created_inbox {
            let _ = std::fs::remove_file(&inbox_path);
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

fn update_member_from_roster_store(
    roster_store: &dyn RosterStore,
    atm_home_dir: &Path,
    request: UpdateMemberRequest,
) -> Result<UpdateMemberOutcome, AtmError> {
    validate_update_member_caller(roster_store, &request)?;
    let team_dir = home::team_dir_from_home(atm_home_dir, &request.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&request.team));
    }

    let current_extra = load_team_projection_extra_for_member_add(&team_dir)?;
    let mut existing_roster = projection::load_team_roster(roster_store, &request.team)?;
    let member_name = request.member.0.clone();
    let member = existing_roster
        .iter_mut()
        .find(|existing_member| existing_member.agent_name == member_name)
        .ok_or_else(|| AtmError::member_not_found(member_name.as_str(), request.team.as_str()))?;

    apply_member_metadata_update(member, &request);
    roster_store
        .replace_roster(&request.team, &existing_roster, None)
        .map_err(|error| {
            error.with_recovery(
                "Check ATM roster store availability and rerun `atm teams update-member`.",
            )
        })?;
    let projected_config =
        projection::project_team_config_from_roster(current_extra, &existing_roster)?;
    filesystem::write_team_config(&team_dir, &projected_config).map_err(|error| {
        error.with_recovery(
            "Check team config permissions and rerun `atm teams update-member`; ATM roster state may already include the repaired metadata.",
        )
    })?;

    Ok(UpdateMemberOutcome {
        action: "update-member",
        team: request.team,
        member: member_name,
    })
}

fn load_member_add_context(
    roster_store: &dyn RosterStore,
    request: &AddMemberRequest,
) -> Result<MemberAddContext, AtmError> {
    let team_dir = home::team_dir_from_home(request.atm_home_dir.as_ref(), &request.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&request.team));
    }

    let current_extra = load_team_projection_extra_for_member_add(&team_dir)?;
    let existing_roster = projection::load_team_roster(roster_store, &request.team)?;
    ensure_member_absent(&existing_roster, &request.team, &request.member)?;
    Ok(MemberAddContext {
        team_dir,
        current_extra,
        existing_roster,
    })
}

fn ensure_member_absent(
    existing_roster: &[RosterEntry],
    team: &TeamName,
    member: &AgentName,
) -> Result<(), AtmError> {
    if existing_roster
        .iter()
        .any(|existing_member| existing_member.agent_name == *member)
    {
        return Err(AtmError::member_already_exists(
            member.as_str(),
            team.as_str(),
        ));
    }
    Ok(())
}

fn validate_update_member_caller(
    roster_store: &dyn RosterStore,
    request: &UpdateMemberRequest,
) -> Result<(), AtmError> {
    if request.caller_team != request.team {
        return Err(AtmError::validation(format!(
            "caller team '{}' does not match update-member target team '{}'",
            request.caller_team, request.team
        ))
        .with_recovery(
            "Run `atm teams update-member` from the target team's ATM shell, or set ATM_TEAM to the same team named in the positional target argument.",
        ));
    }

    let caller_entry = roster_store.query_membership(&request.team, &request.caller_identity)?;
    if caller_entry.is_none() {
        return Err(AtmError::member_not_found(
            request.caller_identity.as_str(),
            request.team.as_str(),
        )
        .with_recovery(
            "Repair the caller roster entry first or rerun `atm teams update-member` as an existing member of the target team.",
        ));
    }

    Ok(())
}

fn build_member_add_roster_record(request: &AddMemberRequest) -> RosterEntry {
    let normalized_tmux_pane_id = request.tmux_pane_id.clone();
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
    extra.insert(
        HOME_DIR_METADATA_KEY.to_string(),
        json!(request.member_home_dir.as_ref().display().to_string()),
    );

    RosterEntry {
        team_name: request.team.clone(),
        agent_name: request.member.clone(),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::ClaudeCode,
        agent_type: request.agent_type.clone(),
        model: request.model.clone(),
        recipient_pane_id: normalized_tmux_pane_id,
        metadata_json: extra,
    }
}

fn replace_roster_for_member_add(
    roster_store: &dyn RosterStore,
    team: &TeamName,
    existing_roster: &[RosterEntry],
) -> Result<(), AtmError> {
    roster_store
        .replace_roster(team, existing_roster, None)
        .map_err(|error| {
            error.with_recovery(
                "Check ATM roster store availability and rerun `atm teams add-member`.",
            )
        })
}

fn apply_member_metadata_update(member: &mut RosterEntry, request: &UpdateMemberRequest) {
    if let Some(home_dir) = &request.home_dir {
        member.metadata_json.insert(
            HOME_DIR_METADATA_KEY.to_string(),
            json!(home_dir.as_ref().display().to_string()),
        );
    }
    if let Some(harness) = request.harness {
        member.harness = harness;
    }
    if let Some(agent_type) = &request.agent_type {
        member.agent_type = agent_type.clone();
    }
    if let Some(model) = &request.model {
        member.model = model.clone();
    }
    if let Some(tmux_pane_id) = &request.tmux_pane_id {
        member.recipient_pane_id = Some(tmux_pane_id.clone());
        member
            .metadata_json
            .insert("backendType".to_string(), json!("tmux"));
        member
            .metadata_json
            .insert("isActive".to_string(), json!(true));
    }
}

/// Create a point-in-time backup of one team's config, inboxes, and task files.
///
/// # Errors
///
/// Returns [`AtmError`] when the team/config is missing or backup directory/file
/// creation fails.
pub fn backup_team_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    request: BackupRequest,
) -> Result<BackupOutcome, AtmError> {
    filesystem::backup_team_from_roster_store(roster_store, request)
}

/// Restore one team from a backup directory.
///
/// # Errors
///
/// Returns [`AtmError`] when backup discovery, staging/live restore work, or
/// config-last persistence fails. Failure to remove the restore marker after a
/// successful restore is degraded to a warning-only follow-up path.
pub fn restore_team_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    request: RestoreRequest,
) -> Result<RestoreResult, AtmError> {
    restore::restore_team_with_roster_store(roster_store, request)
}

pub(crate) fn ordered_roster_member_summaries(
    records: &[RosterEntry],
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
) -> Vec<MemberSummary> {
    projection::ordered_roster_member_summaries(records, caller_identity, live_cwd)
}

const MAX_MEMBER_METADATA_FIELD_LEN: usize = 256;

fn parse_agent_type(value: String) -> Result<AgentType, AtmError> {
    if value.len() > MAX_MEMBER_METADATA_FIELD_LEN {
        return Err(AtmError::validation(format!(
            "agent_type must be at most {MAX_MEMBER_METADATA_FIELD_LEN} bytes"
        )));
    }
    Ok(AgentType::from(value))
}

fn parse_roster_harness(value: String) -> Result<RosterHarness, AtmError> {
    if value.len() > MAX_MEMBER_METADATA_FIELD_LEN {
        return Err(AtmError::validation(format!(
            "harness must be at most {MAX_MEMBER_METADATA_FIELD_LEN} bytes"
        )));
    }
    match value.as_str() {
        "claude-code" => Ok(RosterHarness::ClaudeCode),
        "codex-cli" => Ok(RosterHarness::CodexCli),
        "gemini-cli" => Ok(RosterHarness::GeminiCli),
        "opencode" => Ok(RosterHarness::Opencode),
        _ => Err(AtmError::validation(
            "harness must be one of: claude-code, codex-cli, gemini-cli, opencode".to_string(),
        )),
    }
}

fn load_team_projection_extra_for_member_add(
    team_dir: &Path,
) -> Result<serde_json::Map<String, Value>, AtmError> {
    // Add-member still preserves non-roster Claude config extras while it
    // projects canonical ATM roster truth back into config.json.
    load_claude_team_config_document(team_dir).map(|config| config.extra)
}

fn normalize_tmux_pane_id(pane_id: Option<&str>) -> Result<Option<PaneId>, AtmError> {
    let Some(raw) = pane_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if (raw
        .strip_prefix('%')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit())))
        || raw.chars().all(|ch| ch.is_ascii_digit())
    {
        return PaneId::from_cli(raw).map(Some);
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
    use std::path::PathBuf;
    use std::sync::Mutex;

    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        AddMemberRequest, BackupRequest, MAX_MEMBER_METADATA_FIELD_LEN, MemberName, MembersQuery,
        RestoreRequest, UpdateMemberRequest, add_member_with_roster_store,
        backup_team_with_roster_store, list_members_with_roster_store,
        list_teams_with_roster_store, update_member_with_roster_store,
    };
    use crate::boundary::{
        self, ReplaySource, RosterEntry, RosterHarness, RosterMemberKind, RosterStore,
        RosterStoreHealthSnapshot,
    };
    use crate::error_codes::AtmErrorCode;
    use crate::schema::{HOME_DIR_METADATA_KEY, TeamConfig};
    use crate::test_support::{EnvGuard, ROLE_TEAM_LEAD, TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, TeamName};

    #[derive(Default)]
    struct RecordingRosterStore {
        // Test-only seam: Mutex keeps the fixture simple while serial tests own all access.
        teams: Mutex<BTreeMap<TeamName, Vec<RosterEntry>>>,
    }

    impl RecordingRosterStore {
        fn seed_team(&self, team: &str, members: Vec<RosterEntry>) {
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
            team: &TeamName,
            members: &[RosterEntry],
            _source: Option<&ReplaySource>,
        ) -> Result<(), crate::error::AtmError> {
            self.teams
                .lock()
                .expect("roster store lock")
                .insert(team.clone(), members.to_vec());
            Ok(())
        }

        fn load_roster(&self, team: &TeamName) -> Result<Vec<RosterEntry>, crate::error::AtmError> {
            Ok(self
                .teams
                .lock()
                .expect("roster store lock")
                .get(team)
                .cloned()
                .unwrap_or_default())
        }

        fn query_membership(
            &self,
            team: &TeamName,
            member: &AgentName,
        ) -> Result<Option<RosterEntry>, crate::error::AtmError> {
            Ok(self
                .teams
                .lock()
                .expect("roster store lock")
                .get(team)
                .and_then(|members| {
                    members
                        .iter()
                        .find(|existing| existing.agent_name == *member)
                        .cloned()
                }))
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, crate::error::AtmError> {
            Ok(self
                .teams
                .lock()
                .expect("roster store lock")
                .keys()
                .cloned()
                .collect())
        }

        fn health_snapshot(
            &self,
            team: &TeamName,
        ) -> Result<RosterStoreHealthSnapshot, crate::error::AtmError> {
            let member_count = self
                .teams
                .lock()
                .expect("roster store lock")
                .get(team)
                .map(|members| members.len() as u64)
                .unwrap_or_default();
            Ok(RosterStoreHealthSnapshot {
                team: team.clone(),
                member_count,
                stale: false,
                refreshed_at: None,
            })
        }
    }

    fn roster_member(team: &str, agent: &str) -> RosterEntry {
        RosterEntry {
            team_name: team.parse().expect("team"),
            agent_name: agent.parse().expect("agent"),
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: crate::schema::AgentType::from("worker".to_string()),
            model: crate::types::ModelName::new("gpt-5").expect("model"),
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
    #[serial(team_config_write_env)]
    fn add_member_normalizes_tmux_shape_when_pane_is_provided() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();

        add_member_with_roster_store(
            &roster_store,
            AddMemberRequest {
                atm_home_dir: tempdir.path().to_path_buf().into(),
                team: TEST_TEAM.parse().expect("team"),
                member: TEST_SENDER.parse().expect("member"),
                agent_type: crate::schema::AgentType::from("worker".to_string()),
                model: crate::types::ModelName::new("gpt-5").expect("model"),
                member_home_dir: tempdir.path().to_path_buf().into(),
                tmux_pane_id: Some(crate::types::PaneId::from_cli("7").expect("pane")),
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
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("load roster");
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].recipient_pane_id.as_deref(), Some("%7"));
    }

    #[test]
    #[serial(team_config_write_env)]
    fn add_member_preserves_session_scoped_tmux_target_syntax() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();

        add_member_with_roster_store(
            &roster_store,
            AddMemberRequest {
                atm_home_dir: tempdir.path().to_path_buf().into(),
                team: TEST_TEAM.parse().expect("team"),
                member: TEST_SENDER.parse().expect("member"),
                agent_type: crate::schema::AgentType::from("worker".to_string()),
                model: crate::types::ModelName::new("gpt-5").expect("model"),
                member_home_dir: tempdir.path().to_path_buf().into(),
                tmux_pane_id: Some(crate::types::PaneId::from_cli("session:1.2").expect("pane")),
            },
        )
        .expect("add member");

        let roster = roster_store
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("load roster");
        assert_eq!(roster[0].recipient_pane_id.as_deref(), Some("session:1.2"));
    }

    #[test]
    fn list_members_reports_atm_roster_truth_without_file_members() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        let mut member = roster_member(TEST_TEAM, TEST_SENDER);
        let worker_home_dir = tempdir.path().join("worker-home");
        member.recipient_pane_id = Some(crate::types::PaneId::from_cli("%9").expect("pane"));
        member.metadata_json.insert(
            HOME_DIR_METADATA_KEY.to_string(),
            serde_json::json!(worker_home_dir.display().to_string()),
        );
        roster_store.seed_team(TEST_TEAM, vec![member]);

        let members = list_members_with_roster_store(
            &roster_store,
            MembersQuery {
                team: TEST_TEAM.parse().expect("team"),
                caller_identity: Some(TEST_SENDER.parse().expect("caller")),
                live_cwd: Some(PathBuf::from("/repo/live")),
            },
        )
        .expect("list members");

        assert_eq!(members.team.as_str(), TEST_TEAM);
        assert_eq!(members.members.len(), 1);
        assert_eq!(members.members[0].name.as_str(), TEST_SENDER);
        assert_eq!(members.members[0].tmux_pane_id.as_deref(), Some("%9"));
        assert_eq!(
            members.members[0].home_dir.as_ref(),
            worker_home_dir.as_path()
        );
        assert_eq!(members.members[0].live_cwd.as_deref(), Some("/repo/live"));
    }

    #[test]
    fn project_team_config_from_roster_rejects_invalid_persisted_agent_id() {
        let mut malformed = roster_member(TEST_TEAM, TEST_SENDER);
        malformed
            .metadata_json
            .insert("agentId".to_string(), serde_json::json!("bad/agent/id"));

        let error = super::projection::project_team_config_from_roster(
            serde_json::Map::new(),
            &[malformed],
        )
        .expect_err("invalid persisted agent id");

        assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
        assert!(error.message.contains("invalid persisted agentId"));
        assert!(error.message.contains("bad/agent/id"));
    }

    #[test]
    #[serial(team_config_write_env)]
    fn list_teams_reports_atm_roster_truth() {
        let tempdir = tempdir().expect("tempdir");
        std::fs::write(
            tempdir.path().join(".atm.toml"),
            format!("[atm]\ndefault_team = \"{TEST_TEAM}\"\n"),
        )
        .expect("workspace config");
        let _atm_team = EnvGuard::unset_raw("ATM_TEAM");
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
    #[serial(team_config_write_env)]
    fn add_member_projects_config_from_updated_atm_roster_truth() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        let mut existing = roster_member(TEST_TEAM, ROLE_TEAM_LEAD);
        let lead_home_dir = tempdir.path().join("team-lead-home");
        existing.agent_type = crate::schema::AgentType::from("lead".to_string());
        existing.model = crate::types::ModelName::new("gpt-5").expect("model");
        existing.metadata_json.insert(
            HOME_DIR_METADATA_KEY.to_string(),
            serde_json::json!(lead_home_dir.display().to_string()),
        );
        roster_store.seed_team(TEST_TEAM, vec![existing]);

        add_member_with_roster_store(
            &roster_store,
            AddMemberRequest {
                atm_home_dir: tempdir.path().to_path_buf().into(),
                team: TEST_TEAM.parse().expect("team"),
                member: TEST_SENDER.parse().expect("member"),
                agent_type: crate::schema::AgentType::from("worker".to_string()),
                model: crate::types::ModelName::new("gpt-5").expect("model"),
                member_home_dir: tempdir.path().to_path_buf().into(),
                tmux_pane_id: Some(crate::types::PaneId::from_cli("%12").expect("pane")),
            },
        )
        .expect("add member");

        let roster = roster_store
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("load roster");
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
    #[serial(team_config_write_env)]
    fn update_member_repairs_existing_roster_metadata_and_projects_config() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(
            TEST_TEAM,
            vec![
                roster_member(TEST_TEAM, TEST_SENDER),
                roster_member(TEST_TEAM, ROLE_TEAM_LEAD),
            ],
        );

        update_member_with_roster_store(
            &roster_store,
            tempdir.path(),
            UpdateMemberRequest {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(TEST_SENDER.parse().expect("member")),
                home_dir: Some(PathBuf::from("/repo/worktree").into()),
                harness: Some(RosterHarness::CodexCli),
                agent_type: Some(crate::schema::AgentType::from("worker".to_string())),
                model: Some(crate::types::ModelName::new("gpt-5").expect("model")),
                tmux_pane_id: Some(crate::types::PaneId::from_cli("22").expect("pane")),
            },
        )
        .expect("update member");

        let roster = roster_store
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("load roster");
        assert_eq!(roster.len(), 2);
        let member = roster
            .iter()
            .find(|member| member.agent_name == TEST_SENDER)
            .expect("sender member");
        assert_eq!(member.harness, RosterHarness::CodexCli);
        assert_eq!(member.recipient_pane_id.as_deref(), Some("%22"));
        assert_eq!(
            member.metadata_json.get(HOME_DIR_METADATA_KEY),
            Some(&serde_json::json!("/repo/worktree"))
        );

        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let config: TeamConfig = serde_json::from_slice(
            &std::fs::read(team_dir.join("config.json")).expect("read config"),
        )
        .expect("parse config");
        let projected_member = config
            .members
            .iter()
            .find(|member| member.name == TEST_SENDER)
            .expect("member");
        assert_eq!(projected_member.tmux_pane_id.as_deref(), Some("%22"));
        assert_eq!(
            projected_member.home_dir.as_path(),
            PathBuf::from("/repo/worktree").as_path()
        );
    }

    #[test]
    fn update_member_rejects_caller_team_mismatch_before_mutation() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(TEST_TEAM, vec![roster_member(TEST_TEAM, TEST_SENDER)]);

        let error = update_member_with_roster_store(
            &roster_store,
            tempdir.path(),
            UpdateMemberRequest {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_team: "other-team".parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(TEST_SENDER.parse().expect("member")),
                home_dir: Some(PathBuf::from("/repo/worktree").into()),
                harness: None,
                agent_type: None,
                model: None,
                tmux_pane_id: None,
            },
        )
        .expect_err("caller team mismatch");

        assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
        assert!(error.message.contains("caller team"));
    }

    #[test]
    fn update_member_rejects_caller_missing_from_target_roster() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(TEST_TEAM, vec![roster_member(TEST_TEAM, TEST_RECIPIENT)]);

        let error = update_member_with_roster_store(
            &roster_store,
            tempdir.path(),
            UpdateMemberRequest {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(TEST_RECIPIENT.parse().expect("member")),
                home_dir: Some(PathBuf::from("/repo/worktree").into()),
                harness: None,
                agent_type: None,
                model: None,
                tmux_pane_id: None,
            },
        )
        .expect_err("missing caller");

        assert_eq!(error.code, AtmErrorCode::MemberNotFound);
        assert!(error.message.contains(TEST_SENDER));
    }

    #[test]
    fn update_member_rejects_missing_existing_member() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(TEST_TEAM, vec![roster_member(TEST_TEAM, ROLE_TEAM_LEAD)]);

        let error = update_member_with_roster_store(
            &roster_store,
            tempdir.path(),
            UpdateMemberRequest {
                caller_identity: ROLE_TEAM_LEAD.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(TEST_SENDER.parse().expect("member")),
                home_dir: Some(PathBuf::from("/repo/worktree").into()),
                harness: None,
                agent_type: None,
                model: None,
                tmux_pane_id: None,
            },
        )
        .expect_err("missing member");

        assert_eq!(error.code, AtmErrorCode::MemberNotFound);
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
        let error = super::filesystem::backup_root_from_home(tempdir.path(), "../evil")
            .expect_err("invalid team");

        assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn tasks_dir_from_home_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");
        let error = super::filesystem::tasks_dir_from_home(tempdir.path(), "../evil")
            .expect_err("invalid team");

        assert_eq!(error.code, AtmErrorCode::AddressParseFailed);
    }
}
