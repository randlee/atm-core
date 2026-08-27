#![allow(
    deprecated,
    reason = "team_admin still uses the legacy atm-core roster boundary until the retained admin flows finish migrating to canonical shared storage seams"
)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::boundary::{
    BuiltInNudgeTemplateKind, NudgeTemplateOverrideStore, RosterEntry, RosterHarness, RosterStore,
    TeamNudgeTemplateOverrideMode,
};
use crate::error::AtmError;
use crate::schema::HomeDirPath;
use crate::types::{AgentName, ModelName, PaneId, TeamName};

#[path = "team_admin/filesystem.rs"]
mod filesystem;
#[path = "team_admin/member_mutation.rs"]
mod member_mutation;
#[path = "team_admin/projection.rs"]
mod projection;
#[path = "team_admin/restore.rs"]
mod restore;

pub use member_mutation::{
    AddMemberOutcome, AddMemberRequest, BackendOptions, MemberName, RemoveMemberOutcome,
    RemoveMemberRequest, UpdateMemberOutcome, UpdateMemberRequest, add_member_with_roster_store,
    remove_member_with_roster_store, update_member_with_roster_store,
};

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
    pub harness: RosterHarness,
    pub model: ModelName,
    pub joined_at: Option<u64>,
    pub tmux_pane_id: Option<PaneId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(
        rename = "herdrSession",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub herdr_session: Option<String>,
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

/// Parameters for setting one team-scoped built-in nudge template override.
#[derive(Debug, Clone)]
pub struct SetNudgeTemplateOverrideRequest {
    pub caller_team: TeamName,
    pub team: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
    pub template_body: String,
}

impl SetNudgeTemplateOverrideRequest {
    pub fn new(
        caller_team: TeamName,
        team: &str,
        kind: &str,
        template_body: String,
    ) -> Result<Self, AtmError> {
        validate_nudge_template_body(&template_body)?;
        Ok(Self {
            caller_team,
            team: team.parse()?,
            kind: kind.parse()?,
            template_body,
        })
    }
}

/// Parameters for disabling one team-scoped built-in nudge template.
#[derive(Debug, Clone)]
pub struct DisableNudgeTemplateOverrideRequest {
    pub caller_team: TeamName,
    pub team: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
}

impl DisableNudgeTemplateOverrideRequest {
    pub fn new(caller_team: TeamName, team: &str, kind: &str) -> Result<Self, AtmError> {
        Ok(Self {
            caller_team,
            team: team.parse()?,
            kind: kind.parse()?,
        })
    }
}

/// Parameters for clearing one team-scoped built-in nudge template override.
#[derive(Debug, Clone)]
pub struct ClearNudgeTemplateOverrideRequest {
    pub caller_team: TeamName,
    pub team: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
}

impl ClearNudgeTemplateOverrideRequest {
    pub fn new(caller_team: TeamName, team: &str, kind: &str) -> Result<Self, AtmError> {
        Ok(Self {
            caller_team,
            team: team.parse()?,
            kind: kind.parse()?,
        })
    }
}

/// Result of setting one team-scoped built-in nudge template override.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SetNudgeTemplateOverrideOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
    pub template_body: String,
    pub updated_at: crate::types::IsoTimestamp,
}

/// Result of disabling one team-scoped built-in nudge template override.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DisableNudgeTemplateOverrideOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
    pub updated_at: crate::types::IsoTimestamp,
}

/// Result of clearing one team-scoped built-in nudge template override.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClearNudgeTemplateOverrideOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub kind: BuiltInNudgeTemplateKind,
    pub cleared: bool,
}

/// List teams currently discoverable from canonical ATM roster state.
///
/// # Errors
///
/// Returns [`AtmError`] when the canonical ATM roster store cannot enumerate
/// teams or load roster snapshots for summary counts.
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
/// Returns [`AtmError`] when the canonical ATM roster store cannot load the
/// target team roster or when the team is absent from canonical ATM state.
pub fn list_members_with_roster_store(
    roster_store: &(dyn RosterStore + Send + Sync),
    query: MembersQuery,
) -> Result<MembersList, AtmError> {
    list_members_from_roster_store(roster_store, query)
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

/// Save one team-scoped built-in nudge template override.
///
/// # Errors
///
/// Returns [`AtmError`] when caller-team validation fails or the override row
/// cannot be persisted through the shared override-store boundary.
pub fn set_nudge_template_override_with_store(
    override_store: &(dyn NudgeTemplateOverrideStore + Send + Sync),
    request: SetNudgeTemplateOverrideRequest,
) -> Result<SetNudgeTemplateOverrideOutcome, AtmError> {
    validate_nudge_template_override_team(
        request.caller_team.clone(),
        request.team.clone(),
        "set-nudge-template",
    )?;
    validate_nudge_template_body(&request.template_body)?;

    let row = override_store.save_template_override(
        &request.team,
        request.kind,
        &request.template_body,
    )?;
    let template_body = row
        .template_body()
        .expect("saved override rows must retain template bodies")
        .to_string();
    Ok(SetNudgeTemplateOverrideOutcome {
        action: "set-nudge-template",
        team: row.team_name,
        kind: row.kind,
        template_body,
        updated_at: row.updated_at,
    })
}

/// Disable one team-scoped built-in nudge template override.
///
/// # Errors
///
/// Returns [`AtmError`] when caller-team validation fails or the disabled row
/// cannot be persisted through the shared override-store boundary.
pub fn disable_nudge_template_override_with_store(
    override_store: &(dyn NudgeTemplateOverrideStore + Send + Sync),
    request: DisableNudgeTemplateOverrideRequest,
) -> Result<DisableNudgeTemplateOverrideOutcome, AtmError> {
    validate_nudge_template_override_team(
        request.caller_team,
        request.team.clone(),
        "disable-nudge-template",
    )?;
    let row = override_store.disable_template_override(&request.team, request.kind)?;
    debug_assert!(matches!(row.mode, TeamNudgeTemplateOverrideMode::Disabled));
    Ok(DisableNudgeTemplateOverrideOutcome {
        action: "disable-nudge-template",
        team: row.team_name,
        kind: row.kind,
        updated_at: row.updated_at,
    })
}

/// Clear one team-scoped built-in nudge template override row.
///
/// # Errors
///
/// Returns [`AtmError`] when caller-team validation fails or the clear
/// operation cannot complete through the shared override-store boundary.
pub fn clear_nudge_template_override_with_store(
    override_store: &(dyn NudgeTemplateOverrideStore + Send + Sync),
    request: ClearNudgeTemplateOverrideRequest,
) -> Result<ClearNudgeTemplateOverrideOutcome, AtmError> {
    validate_nudge_template_override_team(
        request.caller_team,
        request.team.clone(),
        "clear-nudge-template",
    )?;
    let cleared = override_store.clear_template_override(&request.team, request.kind)?;
    Ok(ClearNudgeTemplateOverrideOutcome {
        action: "clear-nudge-template",
        team: request.team,
        kind: request.kind,
        cleared,
    })
}

fn validate_nudge_template_override_team(
    caller_team: TeamName,
    team: TeamName,
    _action: &'static str,
) -> Result<(), AtmError> {
    if caller_team != team {
        return Err(AtmError::validation(format!(
            "caller team '{}' does not match nudge-template target team '{}'",
            caller_team, team
        )));
    }
    Ok(())
}

fn validate_nudge_template_body(template_body: &str) -> Result<(), AtmError> {
    if template_body.trim().is_empty() {
        return Err(AtmError::empty_nudge_template_body());
    }
    Ok(())
}

pub(crate) fn ordered_roster_member_summaries(
    records: &[RosterEntry],
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
) -> Vec<MemberSummary> {
    projection::ordered_roster_member_summaries(records, caller_identity, live_cwd)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        AddMemberRequest, BackendOptions, BackupRequest, ClearNudgeTemplateOverrideRequest,
        DisableNudgeTemplateOverrideRequest, MemberName, MembersQuery, RemoveMemberRequest,
        RestoreRequest, SetNudgeTemplateOverrideRequest, UpdateMemberRequest,
        add_member_with_roster_store, backup_team_with_roster_store,
        clear_nudge_template_override_with_store, disable_nudge_template_override_with_store,
        list_members_with_roster_store, list_teams_with_roster_store,
        remove_member_with_roster_store, set_nudge_template_override_with_store,
        update_member_with_roster_store,
    };
    use crate::boundary::{
        self, BuiltInNudgeTemplateKind, NudgeTemplateOverrideStore, RosterEntry, RosterHarness,
        RosterMemberKind, RosterStore, RosterStoreHealthSnapshot, TeamNudgeTemplateOverrideMode,
        TeamNudgeTemplateOverrideRow,
    };
    use crate::error_codes::AtmErrorCode;
    use crate::schema::{HOME_DIR_METADATA_KEY, TeamConfig, WORKSPACE_ROOT_METADATA_KEY};
    use crate::test_support::{
        EnvGuard, ROLE_TEAM_LEAD, TEST_ARCH_CTM, TEST_RECIPIENT, TEST_SENDER, TEST_TEAM,
    };
    use crate::types::{AgentName, TeamName};

    const MAX_MEMBER_METADATA_FIELD_LEN: usize =
        super::member_mutation::MAX_MEMBER_METADATA_FIELD_LEN;

    #[derive(Default)]
    struct RecordingRosterStore {
        // Test-only seam: Mutex keeps the fixture simple while serial tests own all access.
        teams: Mutex<BTreeMap<TeamName, Vec<RosterEntry>>>,
    }

    #[derive(Default)]
    struct RecordingNudgeTemplateOverrideStore {
        rows: Mutex<BTreeMap<(TeamName, BuiltInNudgeTemplateKind), TeamNudgeTemplateOverrideRow>>,
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
    impl atm_storage::contract::sealed::Sealed for RecordingNudgeTemplateOverrideStore {}

    impl RosterStore for RecordingRosterStore {
        fn replace_roster(
            &self,
            team: &TeamName,
            members: &[RosterEntry],
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

    impl NudgeTemplateOverrideStore for RecordingNudgeTemplateOverrideStore {
        fn load_template_override(
            &self,
            team: &TeamName,
            kind: BuiltInNudgeTemplateKind,
        ) -> Result<Option<TeamNudgeTemplateOverrideRow>, crate::error::AtmError> {
            Ok(self
                .rows
                .lock()
                .expect("override store lock")
                .get(&(team.clone(), kind))
                .cloned())
        }

        fn save_template_override(
            &self,
            team: &TeamName,
            kind: BuiltInNudgeTemplateKind,
            template_body: &str,
        ) -> Result<TeamNudgeTemplateOverrideRow, crate::error::AtmError> {
            if template_body.trim().is_empty() {
                return Err(crate::error::AtmError::empty_nudge_template_body());
            }
            let row = TeamNudgeTemplateOverrideRow {
                team_name: team.clone(),
                kind,
                mode: TeamNudgeTemplateOverrideMode::Override {
                    template_body: template_body.to_string(),
                },
                updated_at: crate::types::IsoTimestamp::now(),
            };
            self.rows
                .lock()
                .expect("override store lock")
                .insert((team.clone(), kind), row.clone());
            Ok(row)
        }

        fn disable_template_override(
            &self,
            team: &TeamName,
            kind: BuiltInNudgeTemplateKind,
        ) -> Result<TeamNudgeTemplateOverrideRow, crate::error::AtmError> {
            let row = TeamNudgeTemplateOverrideRow {
                team_name: team.clone(),
                kind,
                mode: TeamNudgeTemplateOverrideMode::Disabled,
                updated_at: crate::types::IsoTimestamp::now(),
            };
            self.rows
                .lock()
                .expect("override store lock")
                .insert((team.clone(), kind), row.clone());
            Ok(row)
        }

        fn clear_template_override(
            &self,
            team: &TeamName,
            kind: BuiltInNudgeTemplateKind,
        ) -> Result<bool, crate::error::AtmError> {
            Ok(self
                .rows
                .lock()
                .expect("override store lock")
                .remove(&(team.clone(), kind))
                .is_some())
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

        assert_eq!(error.code(), AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn explicit_herdr_backend_validates_identity_and_session() {
        let tempdir = tempdir().expect("tempdir");
        let request = AddMemberRequest::new_with_backend(
            tempdir.path().to_path_buf(),
            TEST_TEAM,
            "arch-ctm",
            "worker".into(),
            "gpt-5".into(),
            tempdir.path().to_path_buf(),
            BackendOptions {
                backend: Some("herdr"),
                target: None,
                session: Some("team-a"),
            },
        )
        .expect("Herdr request");
        assert!(request.tmux_pane_id.is_none());
        assert!(matches!(
            request.local_backend,
            Some(crate::delivery_channel::LocalMessageReceivedBackend::Herdr { .. })
        ));

        let error = AddMemberRequest::new_with_backend(
            tempdir.path().to_path_buf(),
            TEST_TEAM,
            "Team-Lead",
            "worker".into(),
            "gpt-5".into(),
            tempdir.path().to_path_buf(),
            BackendOptions {
                backend: Some("herdr"),
                target: None,
                session: None,
            },
        )
        .expect_err("Herdr grammar must be strict");
        assert!(error.message().contains("^[a-z][a-z0-9_-]{0,31}$"));
    }

    #[test]
    fn explicit_herdr_add_persists_mode_without_a_tmux_target() {
        let tempdir = tempdir().expect("tempdir");
        let roster_store = RecordingRosterStore::default();
        let request = AddMemberRequest::new_with_backend(
            tempdir.path().to_path_buf(),
            TEST_TEAM,
            "arch-ctm",
            "worker".into(),
            "gpt-5".into(),
            tempdir.path().to_path_buf(),
            BackendOptions {
                backend: Some("herdr"),
                target: None,
                session: Some("team-a"),
            },
        )
        .expect("Herdr request");
        add_member_with_roster_store(&roster_store, request).expect("add Herdr member");
        let roster = roster_store
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("roster");
        let member = roster
            .iter()
            .find(|member| member.agent_name.as_str() == "arch-ctm")
            .expect("member");
        assert!(member.recipient_pane_id.is_none());
        assert_eq!(
            member.metadata_json.get(concat!("backend", "Type")),
            Some(&serde_json::json!("herdr"))
        );
        assert_eq!(
            member.metadata_json.get("herdrSession"),
            Some(&serde_json::json!("team-a"))
        );
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

        assert_eq!(error.code(), AtmErrorCode::AddressParseFailed);
    }

    #[test]
    #[serial(team_config_write_env)]
    fn add_member_normalizes_tmux_shape_when_pane_is_provided() {
        let tempdir = tempdir().expect("tempdir");
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
                local_backend: None,
                backend_warning: None,
            },
        )
        .expect("add member");

        let roster = roster_store
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("load roster");
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].recipient_pane_id.as_deref(), Some("%7"));
        assert!(
            !tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join(TEST_TEAM)
                .join("config.json")
                .exists()
        );
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
                local_backend: None,
                backend_warning: None,
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
        assert_eq!(members.members[0].harness, RosterHarness::ClaudeCode);
        assert_eq!(
            serde_json::to_value(&members).expect("serialize members")["members"][0]["harness"],
            serde_json::json!("claude-code")
        );
        assert_eq!(RosterHarness::PythonGraft.to_string(), "python-graft");
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

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().contains("invalid persisted agentId"));
        assert!(error.message().contains("bad/agent/id"));
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
    fn add_member_bootstraps_roster_without_team_config_projection() {
        let tempdir = tempdir().expect("tempdir");
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
                local_backend: None,
                backend_warning: None,
            },
        )
        .expect("add member");

        let roster = roster_store
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("load roster");
        assert_eq!(roster.len(), 2);
        assert!(roster.iter().any(|member| member.agent_name == TEST_SENDER));

        assert!(
            !tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join(TEST_TEAM)
                .join("config.json")
                .exists()
        );
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
            UpdateMemberRequest {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(TEST_SENDER.parse().expect("member")),
                home_dir: Some(PathBuf::from("/repo/worktree").into()),
                workspace_root: Some(PathBuf::from("/repo/workspace").into()),
                harness: Some(RosterHarness::CodexCli),
                agent_type: Some(crate::schema::AgentType::from("worker".to_string())),
                model: Some(crate::types::ModelName::new("gpt-5").expect("model")),
                tmux_pane_id: Some(crate::types::PaneId::from_cli("22").expect("pane")),
                local_backend: None,
                backend_warning: None,
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
        assert_eq!(
            member.metadata_json.get(WORKSPACE_ROOT_METADATA_KEY),
            Some(&serde_json::json!("/repo/workspace"))
        );

        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let config: TeamConfig = serde_json::from_slice(
            &std::fs::read(team_dir.join("config.json")).expect("read config"),
        )
        .expect("parse config");
        assert_eq!(config.members.len(), 0);
    }

    #[test]
    #[serial(team_config_write_env)]
    fn update_member_repairs_blank_pane_ids_for_team_lead_and_arch_ctm_fixture() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(
            TEST_TEAM,
            vec![
                roster_member(TEST_TEAM, ROLE_TEAM_LEAD),
                // This fixture must prove the accepted pane-repair flow for a
                // non-lead roster member on the retained line.
                roster_member(TEST_TEAM, TEST_ARCH_CTM),
            ],
        );

        update_member_with_roster_store(
            &roster_store,
            UpdateMemberRequest {
                caller_identity: ROLE_TEAM_LEAD.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(ROLE_TEAM_LEAD.parse().expect("member")),
                home_dir: None,
                workspace_root: None,
                harness: None,
                agent_type: None,
                model: None,
                tmux_pane_id: Some(crate::types::PaneId::from_cli("%0").expect("pane")),
                local_backend: None,
                backend_warning: None,
            },
        )
        .expect("repair team-lead pane");

        update_member_with_roster_store(
            &roster_store,
            UpdateMemberRequest {
                caller_identity: ROLE_TEAM_LEAD.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(TEST_ARCH_CTM.parse().expect("member")),
                home_dir: None,
                workspace_root: None,
                harness: None,
                agent_type: None,
                model: None,
                tmux_pane_id: Some(crate::types::PaneId::from_cli("%1").expect("pane")),
                local_backend: None,
                backend_warning: None,
            },
        )
        .expect("repair secondary member pane");

        let roster = roster_store
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("load roster");
        let team_lead = roster
            .iter()
            .find(|member| member.agent_name.as_str() == ROLE_TEAM_LEAD)
            .expect("lead member");
        let arch_ctm = roster
            .iter()
            .find(|member| member.agent_name.as_str() == TEST_ARCH_CTM)
            .expect("arch fixture member");
        assert_eq!(team_lead.recipient_pane_id.as_deref(), Some("%0"));
        assert_eq!(arch_ctm.recipient_pane_id.as_deref(), Some("%1"));

        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let config: TeamConfig = serde_json::from_slice(
            &std::fs::read(team_dir.join("config.json")).expect("read config"),
        )
        .expect("parse config");
        assert_eq!(config.members.len(), 0);
    }

    #[test]
    fn update_member_rejects_caller_team_mismatch_before_mutation() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(TEST_TEAM, vec![roster_member(TEST_TEAM, TEST_SENDER)]);

        let error = update_member_with_roster_store(
            &roster_store,
            UpdateMemberRequest {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_team: "other-team".parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(TEST_SENDER.parse().expect("member")),
                home_dir: Some(PathBuf::from("/repo/worktree").into()),
                workspace_root: None,
                harness: None,
                agent_type: None,
                model: None,
                tmux_pane_id: None,
                local_backend: None,
                backend_warning: None,
            },
        )
        .expect_err("caller team mismatch");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().contains("caller team"));
    }

    #[test]
    fn update_member_rejects_caller_missing_from_target_roster() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(TEST_TEAM, vec![roster_member(TEST_TEAM, TEST_RECIPIENT)]);

        let error = update_member_with_roster_store(
            &roster_store,
            UpdateMemberRequest {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(TEST_RECIPIENT.parse().expect("member")),
                home_dir: Some(PathBuf::from("/repo/worktree").into()),
                workspace_root: None,
                harness: None,
                agent_type: None,
                model: None,
                tmux_pane_id: None,
                local_backend: None,
                backend_warning: None,
            },
        )
        .expect_err("missing caller");

        assert_eq!(error.code(), AtmErrorCode::MemberNotFound);
        assert!(error.message().contains(TEST_SENDER));
    }

    #[test]
    fn update_member_rejects_missing_existing_member() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(tempdir.path(), TEST_TEAM);
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(TEST_TEAM, vec![roster_member(TEST_TEAM, ROLE_TEAM_LEAD)]);

        let error = update_member_with_roster_store(
            &roster_store,
            UpdateMemberRequest {
                caller_identity: ROLE_TEAM_LEAD.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                team: TEST_TEAM.parse().expect("team"),
                member: MemberName(TEST_SENDER.parse().expect("member")),
                home_dir: Some(PathBuf::from("/repo/worktree").into()),
                workspace_root: None,
                harness: None,
                agent_type: None,
                model: None,
                tmux_pane_id: None,
                local_backend: None,
                backend_warning: None,
            },
        )
        .expect_err("missing member");

        assert_eq!(error.code(), AtmErrorCode::MemberNotFound);
    }

    #[test]
    fn remove_member_removes_exact_roster_entry() {
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(
            TEST_TEAM,
            vec![
                roster_member(TEST_TEAM, ROLE_TEAM_LEAD),
                roster_member(TEST_TEAM, TEST_SENDER),
                roster_member(TEST_TEAM, TEST_RECIPIENT),
            ],
        );

        let outcome = remove_member_with_roster_store(
            &roster_store,
            RemoveMemberRequest::new(
                TEST_SENDER.parse().expect("caller"),
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                TEST_RECIPIENT,
            )
            .expect("request"),
        )
        .expect("remove member");

        assert_eq!(outcome.action, "remove-member");
        assert_eq!(outcome.member.as_str(), TEST_RECIPIENT);
        let roster = roster_store
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("load roster");
        assert_eq!(
            roster
                .iter()
                .map(|member| member.agent_name.as_str())
                .collect::<Vec<_>>(),
            vec![ROLE_TEAM_LEAD, TEST_SENDER]
        );
    }

    #[test]
    fn remove_member_allows_self_removal() {
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(
            TEST_TEAM,
            vec![
                roster_member(TEST_TEAM, TEST_SENDER),
                roster_member(TEST_TEAM, TEST_RECIPIENT),
            ],
        );

        remove_member_with_roster_store(
            &roster_store,
            RemoveMemberRequest::new(
                TEST_SENDER.parse().expect("caller"),
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                TEST_SENDER,
            )
            .expect("request"),
        )
        .expect("self removal");

        let roster = roster_store
            .load_roster(&TEST_TEAM.parse().expect("team"))
            .expect("load roster");
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].agent_name.as_str(), TEST_RECIPIENT);
    }

    #[test]
    fn remove_member_allows_removing_last_member() {
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(TEST_TEAM, vec![roster_member(TEST_TEAM, TEST_SENDER)]);

        remove_member_with_roster_store(
            &roster_store,
            RemoveMemberRequest::new(
                TEST_SENDER.parse().expect("caller"),
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                TEST_SENDER,
            )
            .expect("request"),
        )
        .expect("last-member removal");

        assert!(
            roster_store
                .load_roster(&TEST_TEAM.parse().expect("team"))
                .expect("load roster")
                .is_empty()
        );
    }

    #[test]
    fn remove_member_rejects_missing_member_without_mutation() {
        let roster_store = RecordingRosterStore::default();
        let initial = vec![roster_member(TEST_TEAM, TEST_SENDER)];
        roster_store.seed_team(TEST_TEAM, initial.clone());

        let error = remove_member_with_roster_store(
            &roster_store,
            RemoveMemberRequest::new(
                TEST_SENDER.parse().expect("caller"),
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                TEST_RECIPIENT,
            )
            .expect("request"),
        )
        .expect_err("missing member");

        assert_eq!(error.code(), AtmErrorCode::MemberNotFound);
        assert_eq!(
            roster_store
                .load_roster(&TEST_TEAM.parse().expect("team"))
                .expect("load roster"),
            initial
        );
    }

    #[test]
    fn remove_member_rejects_cross_team_caller_before_mutation() {
        let roster_store = RecordingRosterStore::default();
        let initial = vec![roster_member(TEST_TEAM, TEST_SENDER)];
        roster_store.seed_team(TEST_TEAM, initial.clone());

        let error = remove_member_with_roster_store(
            &roster_store,
            RemoveMemberRequest::new(
                TEST_SENDER.parse().expect("caller"),
                "other-team".parse().expect("caller team"),
                TEST_TEAM,
                TEST_SENDER,
            )
            .expect("request"),
        )
        .expect_err("cross-team caller");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().contains("caller team"));
        assert_eq!(
            roster_store
                .load_roster(&TEST_TEAM.parse().expect("team"))
                .expect("load roster"),
            initial
        );
    }

    #[test]
    fn remove_member_rejects_same_team_missing_caller_before_mutation() {
        let roster_store = RecordingRosterStore::default();
        let initial = vec![roster_member(TEST_TEAM, TEST_RECIPIENT)];
        roster_store.seed_team(TEST_TEAM, initial.clone());

        let error = remove_member_with_roster_store(
            &roster_store,
            RemoveMemberRequest::new(
                TEST_SENDER.parse().expect("caller"),
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                TEST_RECIPIENT,
            )
            .expect("request"),
        )
        .expect_err("missing caller");

        assert_eq!(error.code(), AtmErrorCode::MemberNotFound);
        assert!(error.message().contains(TEST_SENDER));
        assert_eq!(
            roster_store
                .load_roster(&TEST_TEAM.parse().expect("team"))
                .expect("load roster"),
            initial
        );
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
    fn set_nudge_template_override_rejects_invalid_kind() {
        let error = SetNudgeTemplateOverrideRequest::new(
            TEST_TEAM.parse().expect("caller team"),
            TEST_TEAM,
            "not-a-kind",
            "<atm/>".to_string(),
        )
        .expect_err("invalid kind");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(
            error
                .message()
                .contains("unsupported built-in nudge template kind")
        );
    }

    #[test]
    fn set_nudge_template_override_rejects_empty_body() {
        let error = SetNudgeTemplateOverrideRequest::new(
            TEST_TEAM.parse().expect("caller team"),
            TEST_TEAM,
            "delivery_ack",
            "   ".to_string(),
        )
        .expect_err("empty body");

        assert_eq!(error.code(), AtmErrorCode::EmptyNudgeTemplateBody);
    }

    #[test]
    fn set_nudge_template_override_rejects_caller_team_mismatch() {
        let override_store = RecordingNudgeTemplateOverrideStore::default();
        let error = set_nudge_template_override_with_store(
            &override_store,
            SetNudgeTemplateOverrideRequest::new(
                "other-team".parse().expect("caller team"),
                TEST_TEAM,
                "delivery_ack",
                "<atm/>".to_string(),
            )
            .expect("request"),
        )
        .expect_err("caller mismatch");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().contains("caller team"));
    }

    #[test]
    fn set_nudge_template_override_saves_row_through_boundary() {
        let override_store = RecordingNudgeTemplateOverrideStore::default();
        let outcome = set_nudge_template_override_with_store(
            &override_store,
            SetNudgeTemplateOverrideRequest::new(
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                "delivery_ack",
                "<atm/>".to_string(),
            )
            .expect("request"),
        )
        .expect("save");

        assert_eq!(outcome.action, "set-nudge-template");
        assert_eq!(outcome.team.as_str(), TEST_TEAM);
        assert_eq!(outcome.kind, BuiltInNudgeTemplateKind::DeliveryAck);
        assert_eq!(outcome.template_body, "<atm/>");

        let saved = override_store
            .load_template_override(
                &TEST_TEAM.parse().expect("team"),
                BuiltInNudgeTemplateKind::DeliveryAck,
            )
            .expect("load")
            .expect("saved row");
        assert_eq!(saved.template_body(), Some("<atm/>"));
        assert!(!saved.is_disabled());
    }

    #[test]
    fn disable_nudge_template_override_saves_disabled_row_through_boundary() {
        let override_store = RecordingNudgeTemplateOverrideStore::default();
        let outcome = disable_nudge_template_override_with_store(
            &override_store,
            DisableNudgeTemplateOverrideRequest::new(
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                "delivery_ack",
            )
            .expect("request"),
        )
        .expect("disable");

        assert_eq!(outcome.action, "disable-nudge-template");
        assert_eq!(outcome.team.as_str(), TEST_TEAM);
        assert_eq!(outcome.kind, BuiltInNudgeTemplateKind::DeliveryAck);

        let saved = override_store
            .load_template_override(
                &TEST_TEAM.parse().expect("team"),
                BuiltInNudgeTemplateKind::DeliveryAck,
            )
            .expect("load")
            .expect("saved row");
        assert!(saved.is_disabled());
        assert_eq!(saved.template_body(), None);
    }

    #[test]
    fn clear_nudge_template_override_deletes_row_and_reports_state() {
        let override_store = RecordingNudgeTemplateOverrideStore::default();
        set_nudge_template_override_with_store(
            &override_store,
            SetNudgeTemplateOverrideRequest::new(
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                "delivery_ack",
                "<atm/>".to_string(),
            )
            .expect("request"),
        )
        .expect("seed");

        let outcome = clear_nudge_template_override_with_store(
            &override_store,
            ClearNudgeTemplateOverrideRequest::new(
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                "delivery_ack",
            )
            .expect("request"),
        )
        .expect("clear");
        assert!(outcome.cleared);

        let saved = override_store
            .load_template_override(
                &TEST_TEAM.parse().expect("team"),
                BuiltInNudgeTemplateKind::DeliveryAck,
            )
            .expect("load");
        assert!(saved.is_none());

        let second = clear_nudge_template_override_with_store(
            &override_store,
            ClearNudgeTemplateOverrideRequest::new(
                TEST_TEAM.parse().expect("caller team"),
                TEST_TEAM,
                "delivery_ack",
            )
            .expect("request"),
        )
        .expect("clear missing");
        assert!(!second.cleared);
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

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().contains("model"));
    }

    #[test]
    fn backup_team_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");

        let error =
            BackupRequest::new(tempdir.path().to_path_buf(), "../evil").expect_err("invalid team");

        assert_eq!(error.code(), AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn restore_team_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");

        let error = RestoreRequest::new(tempdir.path().to_path_buf(), "../evil", None, false)
            .expect_err("invalid team");

        assert_eq!(error.code(), AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn backup_root_from_home_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");
        let error = super::filesystem::backup_root_from_home(tempdir.path(), "../evil")
            .expect_err("invalid team");

        assert_eq!(error.code(), AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn tasks_dir_from_home_rejects_invalid_team_segment() {
        let tempdir = tempdir().expect("tempdir");
        let error = super::filesystem::tasks_dir_from_home(tempdir.path(), "../evil")
            .expect_err("invalid team");

        assert_eq!(error.code(), AtmErrorCode::AddressParseFailed);
    }
}
