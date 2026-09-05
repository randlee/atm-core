#![allow(
    deprecated,
    reason = "team_admin restore still uses the legacy atm-core roster boundary until the retained admin flows finish migrating to canonical shared storage seams"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing::warn;

use crate::boundary::RosterStore;
use crate::error::{AtmError, AtmErrorCode};
use crate::home;
use crate::persistence;
use crate::roles::ROLE_TEAM_LEAD;

use super::{RestoreOutcome, RestorePlan, RestoreRequest, RestoreResult};

struct RestoreExecutionPlan {
    team_dir: PathBuf,
    backup_dir: PathBuf,
    members_to_restore: Vec<crate::types::AgentName>,
    inboxes_to_restore: Vec<String>,
    tasks_to_restore: usize,
}

pub(super) fn restore_team_with_roster_store(
    roster_store: &dyn RosterStore,
    request: RestoreRequest,
) -> Result<RestoreResult, AtmError> {
    let plan = build_restore_execution_plan(roster_store, &request)?;

    if request.dry_run {
        return Ok(build_restore_dry_run(&request.team, &plan));
    }

    prepare_restore_workspace(&plan.team_dir, &plan.backup_dir)?;
    let outcome = match apply_restore_execution_plan(&request, &plan) {
        Ok(outcome) => outcome,
        Err(error) => return restore_with_cleanup_failure(&request.team, &plan.team_dir, error),
    };

    let marker_cleanup_error = clear_restore_marker(&plan.team_dir).err();
    cleanup_restore_workspace(&plan.team_dir)?;
    if let Some(error) = marker_cleanup_error {
        warn!(
            code = %AtmErrorCode::WarningRestoreInProgress,
            %error,
            team = %request.team,
            "restore completed but the stale restore marker could not be removed"
        );
    }

    Ok(RestoreResult::Applied(outcome))
}

fn build_restore_execution_plan(
    roster_store: &dyn RosterStore,
    request: &RestoreRequest,
) -> Result<RestoreExecutionPlan, AtmError> {
    let team_dir = home::team_dir_from_home(&request.home_dir, &request.team)?;
    let canonical_roster = super::projection::load_team_roster(roster_store, &request.team)?;
    if canonical_roster.is_empty() {
        return Err(AtmError::team_not_found(&request.team));
    }

    let backup_dir = locate_backup_dir(&request.home_dir, &request.team, request.from.as_deref())?;
    let members_to_restore = canonical_roster
        .iter()
        .filter(|member| member.agent_name != ROLE_TEAM_LEAD)
        .map(|member| member.agent_name.clone())
        .collect::<Vec<_>>();
    let inboxes_to_restore = filter_restore_inboxes(&backup_dir, &members_to_restore)?;
    let tasks_to_restore = count_numeric_task_files(&backup_dir.join("tasks"))?;

    Ok(RestoreExecutionPlan {
        team_dir,
        backup_dir,
        members_to_restore,
        inboxes_to_restore,
        tasks_to_restore,
    })
}

fn filter_restore_inboxes(
    backup_dir: &Path,
    members_to_restore: &[crate::types::AgentName],
) -> Result<Vec<String>, AtmError> {
    let members_to_restore_set = members_to_restore.iter().cloned().collect::<BTreeSet<_>>();
    let mut inboxes_to_restore = list_backup_inboxes(backup_dir)?;
    inboxes_to_restore.retain(|name| {
        if name == &format!("{ROLE_TEAM_LEAD}.json") {
            return false;
        }
        name.strip_suffix(".json").is_some_and(|member| {
            members_to_restore_set
                .iter()
                .any(|restored_member| restored_member.as_str() == member)
        })
    });
    Ok(inboxes_to_restore)
}

fn build_restore_dry_run(
    team: &crate::types::TeamName,
    plan: &RestoreExecutionPlan,
) -> RestoreResult {
    RestoreResult::DryRun(RestorePlan {
        action: "restore",
        team: team.clone(),
        backup_path: plan.backup_dir.clone(),
        dry_run: true,
        would_restore_members: plan
            .members_to_restore
            .iter()
            .cloned()
            .map(crate::types::AgentName::from_validated)
            .collect(),
        would_restore_inboxes: plan.inboxes_to_restore.clone(),
        would_restore_tasks: plan.tasks_to_restore,
    })
}

fn apply_restore_execution_plan(
    request: &RestoreRequest,
    plan: &RestoreExecutionPlan,
) -> Result<RestoreOutcome, AtmError> {
    apply_restored_inboxes(&plan.team_dir, &plan.backup_dir, &plan.inboxes_to_restore)?;

    let tasks_dir = super::filesystem::tasks_dir_from_home(&request.home_dir, &request.team)?;
    restore_task_state_from_backup(&plan.backup_dir.join("tasks"), &tasks_dir)?;

    Ok(RestoreOutcome {
        action: "restore",
        team: request.team.clone(),
        backup_path: plan.backup_dir.clone(),
        members_restored: plan.members_to_restore.len(),
        inboxes_restored: plan.inboxes_to_restore.len(),
        tasks_restored: plan.tasks_to_restore,
    })
}

fn restore_with_cleanup_failure(
    team: &crate::types::TeamName,
    team_dir: &Path,
    error: AtmError,
) -> Result<RestoreResult, AtmError> {
    if let Err(cleanup_error) = cleanup_restore_workspace(team_dir) {
        warn!(
            team = %team,
            %cleanup_error,
            "restore failed and cleanup of the restore staging directory also failed"
        );
    }
    Err(error)
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

    let root = super::filesystem::backup_root_from_home(home_dir, team)?;
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
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read backup directory entry under {}: {error}",
                root.display()
            ))
        })?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    entries.sort();
    entries
        .pop()
        .ok_or_else(|| AtmError::missing_document(format!("no backup found for team '{}'", team)))
}

pub(super) fn list_backup_inboxes(backup_dir: &Path) -> Result<Vec<String>, AtmError> {
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
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AtmError::mailbox_read(format!(
                "failed to read backup inbox directory entry under {}: {error}",
                inbox_dir.display()
            ))
        })?
        .into_iter()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

pub(super) fn count_numeric_task_files(tasks_dir: &Path) -> Result<usize, AtmError> {
    if !tasks_dir.exists() {
        return Ok(0);
    }
    let count = fs::read_dir(tasks_dir)
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read task directory {}: {error}",
                tasks_dir.display()
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read task directory entry under {}: {error}",
                tasks_dir.display()
            ))
        })?
        .into_iter()
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
        })?;
    }
    super::filesystem::copy_regular_files_strict(src, &staging, |name| {
        name == ".highwatermark" || name.ends_with(".json")
    })?;

    if dst.exists() {
        fs::remove_dir_all(dst).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to clear existing task directory {}: {error}",
                dst.display()
            ))
        })?;
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to create task parent directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    fs::rename(&staging, dst).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to install restored task directory {}: {error}",
            dst.display()
        ))
    })?;
    Ok(())
}

fn recompute_highwatermark(tasks_dir: &Path) -> Result<usize, AtmError> {
    fs::create_dir_all(tasks_dir).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to create task directory {}: {error}",
            tasks_dir.display()
        ))
    })?;

    let max_id = fs::read_dir(tasks_dir)
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read task directory {}: {error}",
                tasks_dir.display()
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read task directory entry under {}: {error}",
                tasks_dir.display()
            ))
        })?
        .into_iter()
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

    persistence::atomic_write_string(
        &tasks_dir.join(".highwatermark"),
        &format!("{max_id}\n"),
        AtmErrorCode::FilePolicyRejected,
        "task highwatermark",
        "Check task directory permissions and rerun the restore.",
    )?;
    Ok(max_id)
}

fn restore_marker_path(team_dir: &Path) -> PathBuf {
    team_dir.join(".restore-in-progress")
}

fn restore_staging_dir(team_dir: &Path) -> PathBuf {
    team_dir.join(".restore-staging")
}

fn restore_staging_inboxes_dir(team_dir: &Path) -> PathBuf {
    restore_staging_dir(team_dir).join("inboxes")
}

fn prepare_restore_staging_dir(team_dir: &Path) -> Result<(), AtmError> {
    let staging_root = restore_staging_dir(team_dir);
    if staging_root.exists() {
        return Err(AtmError::file_policy(format!(
            "restore staging directory already exists at {}",
            staging_root.display()
        )));
    }
    Ok(())
}

fn copy_restored_inbox_to_staging(from: &Path, staged: &Path) -> Result<u64, std::io::Error> {
    if std::env::var_os("ATM_TEST_FAIL_RESTORE_INBOX_STAGE").is_some() {
        return Err(std::io::Error::other(format!(
            "forced inbox staging failure for {}",
            staged.display()
        )));
    }
    fs::copy(from, staged)
}

pub(super) fn prepare_restore_workspace(
    team_dir: &Path,
    backup_dir: &Path,
) -> Result<(), AtmError> {
    prepare_restore_staging_dir(team_dir)?;
    write_restore_marker(team_dir, backup_dir)
}

pub(super) fn cleanup_restore_workspace(team_dir: &Path) -> Result<(), AtmError> {
    let staging_root = restore_staging_dir(team_dir);
    if !staging_root.exists() {
        return Ok(());
    }

    fs::remove_dir_all(&staging_root).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to remove restore staging directory {}: {error}",
            staging_root.display()
        ))
    })
}

pub(super) fn apply_restored_inboxes(
    team_dir: &Path,
    backup_dir: &Path,
    inboxes_to_restore: &[String],
) -> Result<(), AtmError> {
    let inboxes_dir = team_dir.join("inboxes");
    fs::create_dir_all(&inboxes_dir).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create inbox directory {}: {error}",
            inboxes_dir.display()
        ))
    })?;
    let inbox_staging_dir = restore_staging_inboxes_dir(team_dir);
    fs::create_dir_all(&inbox_staging_dir).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create inbox restore staging directory {}: {error}",
            inbox_staging_dir.display()
        ))
    })?;
    for inbox_name in inboxes_to_restore {
        let from = backup_dir.join("inboxes").join(inbox_name);
        let staged = inbox_staging_dir.join(inbox_name);
        copy_restored_inbox_to_staging(&from, &staged).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to stage restored inbox {} from {}: {error}",
                staged.display(),
                from.display()
            ))
        })?;
    }
    for inbox_name in inboxes_to_restore {
        let staged = inbox_staging_dir.join(inbox_name);
        let to = inboxes_dir.join(inbox_name);
        fs::rename(&staged, &to).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to install restored inbox {} from {}: {error}",
                to.display(),
                staged.display()
            ))
        })?;
    }
    Ok(())
}

pub(super) fn restore_task_state_from_backup(
    backup_tasks_dir: &Path,
    tasks_dir: &Path,
) -> Result<usize, AtmError> {
    restore_task_bucket(backup_tasks_dir, tasks_dir)?;
    recompute_highwatermark(tasks_dir)
}

fn write_restore_marker(team_dir: &Path, backup_dir: &Path) -> Result<(), AtmError> {
    let marker = restore_marker_path(team_dir);
    let payload = serde_json::json!({
        "backup_path": backup_dir,
        "pid": std::process::id(),
        "timestamp": Utc::now().to_rfc3339(),
    });
    let bytes = serde_json::to_vec_pretty(&payload).map_err(AtmError::from)?;
    persistence::atomic_write_bytes(
        &marker,
        &bytes,
        AtmErrorCode::FilePolicyRejected,
        "restore marker",
        "Check team directory permissions and rerun `atm teams restore`.",
    )
}

pub(super) fn clear_restore_marker(team_dir: &Path) -> Result<(), AtmError> {
    let marker = restore_marker_path(team_dir);
    if !marker.exists() {
        return Ok(());
    }

    if std::env::var_os("ATM_TEST_FAIL_RESTORE_MARKER_REMOVE").is_some() {
        return Err(AtmError::file_policy(format!(
            "failed to remove restore marker {}: forced test failure",
            marker.display()
        )));
    }

    fs::remove_file(&marker).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to remove restore marker {}: {error}",
            marker.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;

    use chrono::Utc;
    use serde_json::json;
    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        clear_restore_marker, prepare_restore_workspace, restore_marker_path, restore_staging_dir,
        restore_task_state_from_backup, restore_team_with_roster_store,
    };
    use crate::boundary::{
        self, RosterEntry, RosterHarness, RosterMemberKind, RosterStore, RosterStoreHealthSnapshot,
    };
    use crate::error::AtmError;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::TeamConfig;
    use crate::team_admin::{RestoreRequest, RestoreResult};
    use crate::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, TeamName};

    #[derive(Default)]
    struct RecordingRosterStore {
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
        fn replace_roster(&self, team: &TeamName, members: &[RosterEntry]) -> Result<(), AtmError> {
            self.teams
                .lock()
                .expect("roster store lock")
                .insert(team.clone(), members.to_vec());
            Ok(())
        }

        fn load_roster(&self, team: &TeamName) -> Result<Vec<RosterEntry>, AtmError> {
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
        ) -> Result<Option<RosterEntry>, AtmError> {
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

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            Ok(self
                .teams
                .lock()
                .expect("roster store lock")
                .keys()
                .cloned()
                .collect())
        }

        fn health_snapshot(&self, team: &TeamName) -> Result<RosterStoreHealthSnapshot, AtmError> {
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
            agent_type: crate::schema::AgentType::default(),
            model: crate::types::ModelName::default(),
            recipient_pane_id: None,
            metadata_json: serde_json::Map::new(),
        }
    }

    fn write_team_config(home_dir: &Path, team: &str, value: serde_json::Value) {
        write_json(
            &home_dir
                .join(".claude")
                .join("teams")
                .join(team)
                .join("config.json"),
            &value,
        );
    }

    fn write_text(path: &Path, value: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent dir");
        }
        fs::write(path, value).expect("write text");
    }

    fn write_json(path: &Path, value: &serde_json::Value) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent dir");
        }
        fs::write(path, serde_json::to_vec_pretty(value).expect("json")).expect("write json");
    }

    fn write_inbox(path: &Path, text: &str) {
        let envelope = crate::schema::InboxMessage {
            from: ROLE_TEAM_LEAD.parse().expect("agent"),
            source_chat_id: None,
            text: text.to_string(),
            timestamp: crate::types::IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some(TEST_TEAM.parse().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: None,
            requires_ack: false,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            task_complete: None,
            extra: serde_json::Map::new(),
        };
        write_text(
            path,
            &format!("{}\n", serde_json::to_string(&envelope).expect("envelope")),
        );
    }

    fn with_env_var_serial<T>(key: &'static str, value: &str, body: impl FnOnce() -> T) -> T {
        let _env_guard = crate::test_support::EnvGuard::set_raw(key, value);
        body()
    }

    #[test]
    fn prepare_restore_workspace_rejects_preexisting_staging_dir() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let backup_dir = tempdir.path().join("backup");
        fs::create_dir_all(restore_staging_dir(&team_dir)).expect("staging dir");
        fs::create_dir_all(&backup_dir).expect("backup dir");

        let error = prepare_restore_workspace(&team_dir, &backup_dir).expect_err("staging error");

        assert!(error.code() == crate::error_codes::AtmErrorCode::FilePolicyRejected);
        assert!(
            error
                .message()
                .contains("restore staging directory already exists")
        );
        assert!(!restore_marker_path(&team_dir).exists());
    }

    #[test]
    fn restore_task_state_from_backup_round_trips_highwatermark() {
        let tempdir = tempdir().expect("tempdir");
        let backup_tasks_dir = tempdir.path().join("backup").join("tasks");
        let tasks_dir = tempdir.path().join(".claude").join("tasks").join(TEST_TEAM);
        write_json(
            &backup_tasks_dir.join("2.json"),
            &json!({"id":"2","status":"open"}),
        );
        write_json(
            &backup_tasks_dir.join("9.json"),
            &json!({"id":"9","status":"open"}),
        );
        write_text(&backup_tasks_dir.join(".highwatermark"), "1\n");
        write_json(
            &tasks_dir.join("1.json"),
            &json!({"id":"1","status":"open"}),
        );
        write_text(&tasks_dir.join(".highwatermark"), "1\n");

        let highwatermark =
            restore_task_state_from_backup(&backup_tasks_dir, &tasks_dir).expect("restore tasks");

        assert_eq!(highwatermark, 9);
        assert!(!tasks_dir.join("1.json").exists());
        assert!(tasks_dir.join("2.json").is_file());
        assert!(tasks_dir.join("9.json").is_file());
        assert_eq!(
            fs::read_to_string(tasks_dir.join(".highwatermark")).expect("highwatermark"),
            "9\n"
        );
    }

    #[test]
    #[serial(team_config_write_env)]
    fn restore_team_ignores_legacy_config_write_failure_hook() {
        let tempdir = tempdir().expect("tempdir");
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(
            TEST_TEAM,
            vec![
                roster_member(TEST_TEAM, ROLE_TEAM_LEAD),
                roster_member(TEST_TEAM, TEST_SENDER),
            ],
        );
        write_team_config(
            tempdir.path(),
            TEST_TEAM,
            json!({"leadSessionId":"lead-current","members":[{"name":ROLE_TEAM_LEAD}]}),
        );
        let backup_dir = tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(".backups")
            .join(TEST_TEAM)
            .join("20260423T010203000000000Z");
        write_inbox(
            &backup_dir
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json")),
            "restored worker inbox",
        );
        write_json(
            &backup_dir.join("tasks").join("80.json"),
            &json!({"id":"80"}),
        );

        let result = with_env_var_serial("ATM_TEST_FAIL_TEAM_CONFIG_WRITE", "1", || {
            restore_team_with_roster_store(
                &roster_store,
                RestoreRequest {
                    home_dir: tempdir.path().to_path_buf(),
                    team: TEST_TEAM.parse().expect("team"),
                    from: Some(backup_dir.clone()),
                    dry_run: false,
                },
            )
        });

        let outcome = result.expect("restore succeeds without config writes");
        match outcome {
            RestoreResult::Applied(outcome) => {
                assert_eq!(outcome.members_restored, 1);
                assert_eq!(outcome.inboxes_restored, 1);
                assert_eq!(outcome.tasks_restored, 1);
            }
            RestoreResult::DryRun(_) => panic!("expected applied restore"),
        }
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        assert!(
            team_dir
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json"))
                .is_file()
        );
        assert!(
            tempdir
                .path()
                .join(".claude")
                .join("tasks")
                .join(TEST_TEAM)
                .join("80.json")
                .is_file()
        );
        assert!(!restore_marker_path(&team_dir).is_file());
    }

    #[test]
    #[serial(team_config_write_env)]
    fn restore_team_treats_marker_cleanup_failure_as_warning_only() {
        let tempdir = tempdir().expect("tempdir");
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(
            TEST_TEAM,
            vec![
                roster_member(TEST_TEAM, ROLE_TEAM_LEAD),
                roster_member(TEST_TEAM, TEST_SENDER),
            ],
        );
        write_team_config(
            tempdir.path(),
            TEST_TEAM,
            json!({"leadSessionId":"lead-current","members":[{"name":ROLE_TEAM_LEAD}]}),
        );
        let backup_dir = tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(".backups")
            .join(TEST_TEAM)
            .join("20260423T020304000000000Z");
        write_inbox(
            &backup_dir
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json")),
            "restored worker inbox",
        );

        let result = with_env_var_serial("ATM_TEST_FAIL_RESTORE_MARKER_REMOVE", "1", || {
            restore_team_with_roster_store(
                &roster_store,
                RestoreRequest {
                    home_dir: tempdir.path().to_path_buf(),
                    team: TEST_TEAM.parse().expect("team"),
                    from: Some(backup_dir.clone()),
                    dry_run: false,
                },
            )
        });

        assert!(
            result.is_ok(),
            "restore should succeed despite marker cleanup"
        );
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        assert!(restore_marker_path(&team_dir).is_file());
        assert!(
            team_dir
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json"))
                .is_file()
        );
    }

    #[test]
    #[serial(team_config_write_env)]
    fn restore_team_cleans_staging_and_preserves_live_config_on_inbox_stage_failure() {
        let tempdir = tempdir().expect("tempdir");
        let roster_store = RecordingRosterStore::default();
        roster_store.seed_team(
            TEST_TEAM,
            vec![
                roster_member(TEST_TEAM, ROLE_TEAM_LEAD),
                roster_member(TEST_TEAM, TEST_SENDER),
            ],
        );
        write_team_config(
            tempdir.path(),
            TEST_TEAM,
            json!({"leadSessionId":"lead-current","members":[{"name":ROLE_TEAM_LEAD}]}),
        );
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let backup_dir = tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(".backups")
            .join(TEST_TEAM)
            .join("20260424T022700000000000Z");
        write_inbox(
            &backup_dir
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json")),
            "restored worker inbox",
        );

        let result = with_env_var_serial("ATM_TEST_FAIL_RESTORE_INBOX_STAGE", "1", || {
            restore_team_with_roster_store(
                &roster_store,
                RestoreRequest {
                    home_dir: tempdir.path().to_path_buf(),
                    team: TEST_TEAM.parse().expect("team"),
                    from: Some(backup_dir.clone()),
                    dry_run: false,
                },
            )
        });

        let error = result.expect_err("restore should fail on injected inbox stage error");
        assert_eq!(
            error.code(),
            crate::error_codes::AtmErrorCode::MailboxWriteFailed
        );
        assert!(!restore_staging_dir(&team_dir).exists());
        assert!(
            !team_dir
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json"))
                .exists()
        );
        assert!(restore_marker_path(&team_dir).is_file());
    }

    #[test]
    #[serial(team_config_write_env)]
    fn restore_team_no_longer_projects_config_from_roster() {
        let tempdir = tempdir().expect("tempdir");
        let roster_store = RecordingRosterStore::default();
        let mut recipient = roster_member(TEST_TEAM, TEST_RECIPIENT);
        recipient.recipient_pane_id = Some(crate::types::PaneId::from_cli("%12").expect("pane"));
        recipient
            .metadata_json
            .insert("home_dir".to_string(), json!("/repo/recipient"));
        roster_store.seed_team(
            TEST_TEAM,
            vec![
                roster_member(TEST_TEAM, ROLE_TEAM_LEAD),
                roster_member(TEST_TEAM, TEST_SENDER),
                recipient,
            ],
        );
        write_team_config(
            tempdir.path(),
            TEST_TEAM,
            json!({
                "leadSessionId":"lead-current",
                "members":[{"name":ROLE_TEAM_LEAD,"agentType":"lead"}]
            }),
        );
        let backup_dir = tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(".backups")
            .join(TEST_TEAM)
            .join("20260424T030405000000000Z");
        write_inbox(
            &backup_dir
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json")),
            "restored sender inbox",
        );
        write_inbox(
            &backup_dir
                .join("inboxes")
                .join(format!("{TEST_RECIPIENT}.json")),
            "restored recipient inbox",
        );
        write_json(
            &backup_dir.join("tasks").join("80.json"),
            &json!({"id":"80"}),
        );

        let outcome = restore_team_with_roster_store(
            &roster_store,
            RestoreRequest {
                home_dir: tempdir.path().to_path_buf(),
                team: TEST_TEAM.parse().expect("team"),
                from: Some(backup_dir),
                dry_run: false,
            },
        )
        .expect("restore succeeds without backup config");

        match outcome {
            RestoreResult::Applied(outcome) => {
                assert_eq!(outcome.members_restored, 2);
                assert_eq!(outcome.inboxes_restored, 2);
                assert_eq!(outcome.tasks_restored, 1);
            }
            RestoreResult::DryRun(_) => panic!("expected applied restore"),
        }

        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let config: TeamConfig =
            serde_json::from_slice(&fs::read(team_dir.join("config.json")).expect("config"))
                .expect("parse config");
        assert_eq!(config.members.len(), 1);
        assert_eq!(config.members[0].name, ROLE_TEAM_LEAD);
        assert_eq!(config.extra["leadSessionId"], json!("lead-current"));
    }

    #[test]
    fn clear_restore_marker_missing_file_is_ok() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        fs::create_dir_all(&team_dir).expect("team dir");

        clear_restore_marker(&team_dir).expect("missing marker should be ok");
    }
}
