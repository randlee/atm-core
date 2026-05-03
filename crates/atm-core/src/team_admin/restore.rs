use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing::warn;

use crate::config::load_team_config;
use crate::error::{AtmError, AtmErrorCode, AtmErrorKind};
use crate::home;
use crate::mailbox::lock;
use crate::persistence;
use crate::schema::AgentMember;

use super::{RestoreOutcome, RestorePlan, RestoreRequest, RestoreResult};

pub(super) fn restore_team(request: RestoreRequest) -> Result<RestoreResult, AtmError> {
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
    let members_to_restore_set = members_to_restore.iter().cloned().collect::<BTreeSet<_>>();

    let mut inboxes_to_restore = list_backup_inboxes(&backup_dir)?;
    inboxes_to_restore.retain(|name| {
        if name == "team-lead.json" {
            return false;
        }
        name.strip_suffix(".json")
            .is_some_and(|member| members_to_restore_set.contains(member))
    });
    let tasks_to_restore = count_numeric_task_files(&backup_dir.join("tasks"))?;

    if request.dry_run {
        return Ok(RestoreResult::DryRun(RestorePlan {
            action: "restore",
            team: request.team.clone(),
            backup_path: backup_dir,
            dry_run: true,
            would_restore_members: members_to_restore
                .into_iter()
                .map(crate::types::AgentName::from_validated)
                .collect(),
            would_restore_inboxes: inboxes_to_restore,
            would_restore_tasks: tasks_to_restore,
        }));
    }

    prepare_restore_workspace(&team_dir, &backup_dir)?;
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

    let restore_result = (|| {
        apply_restored_inboxes(&team_dir, &backup_dir, &inboxes_to_restore)?;

        let tasks_dir = super::tasks_dir_from_home(&request.home_dir, &request.team)?;
        restore_task_state_from_backup(&backup_dir.join("tasks"), &tasks_dir)?;
        super::write_team_config(&team_dir, &updated_config).map_err(|error| {
            error.with_recovery("Check team config permissions and rerun `atm teams restore`.")
        })?;

        Ok::<RestoreOutcome, AtmError>(RestoreOutcome {
            action: "restore",
            team: request.team.clone(),
            backup_path: backup_dir.clone(),
            members_restored: members_to_restore.len(),
            inboxes_restored: inboxes_to_restore.len(),
            tasks_restored: tasks_to_restore,
        })
    })();
    let outcome = match restore_result {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Err(cleanup_error) = cleanup_restore_workspace(&team_dir) {
                warn!(
                    team = %request.team,
                    %cleanup_error,
                    "restore failed and cleanup of the restore staging directory also failed"
                );
            }
            return Err(error);
        }
    };

    let marker_cleanup_error = clear_restore_marker(&team_dir).err();
    cleanup_restore_workspace(&team_dir)?;
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

    let root = super::backup_root_from_home(home_dir, team)?;
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
            .with_recovery("Check backup directory permissions or pass an explicit --from path.")
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read backup directory entry under {}: {error}",
                root.display()
            ))
            .with_source(error)
            .with_recovery("Check backup directory permissions or pass an explicit --from path.")
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
            .with_source(error)
            .with_recovery("Check backup inbox permissions and retry the restore.")
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AtmError::mailbox_read(format!(
                "failed to read backup inbox directory entry under {}: {error}",
                inbox_dir.display()
            ))
            .with_source(error)
            .with_recovery("Check backup inbox permissions and retry the restore.")
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
            .with_source(error)
            .with_recovery("Check task directory permissions and retry the restore.")
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read task directory entry under {}: {error}",
                tasks_dir.display()
            ))
            .with_source(error)
            .with_recovery("Check task directory permissions and retry the restore.")
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

pub(super) fn clear_runtime_member_state(member: &mut AgentMember) {
    member.tmux_pane_id.clear();
    for key in [
        "backendType",
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

fn restore_task_bucket(src: &Path, dst: &Path) -> Result<(), AtmError> {
    if !src.exists() {
        fs::create_dir_all(dst).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to create task directory {}: {error}",
                dst.display()
            ))
            .with_source(error)
            .with_recovery("Check task directory permissions and rerun the restore.")
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
            .with_recovery("Check task staging directory permissions and rerun the restore.")
        })?;
    }
    super::copy_regular_files_strict(src, &staging, |name| {
        name == ".highwatermark" || name.ends_with(".json")
    })?;

    if dst.exists() {
        fs::remove_dir_all(dst).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to clear existing task directory {}: {error}",
                dst.display()
            ))
            .with_source(error)
            .with_recovery("Check task directory permissions and rerun the restore.")
        })?;
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::file_policy(format!(
                "failed to create task parent directory {}: {error}",
                parent.display()
            ))
            .with_source(error)
            .with_recovery("Check task parent directory permissions and rerun the restore.")
        })?;
    }
    fs::rename(&staging, dst).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to install restored task directory {}: {error}",
            dst.display()
        ))
        .with_source(error)
        .with_recovery("Check task directory permissions and rerun the restore.")
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
        .with_recovery("Check task directory permissions and rerun the restore.")
    })?;

    let max_id = fs::read_dir(tasks_dir)
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read task directory {}: {error}",
                tasks_dir.display()
            ))
            .with_source(error)
            .with_recovery("Check task directory permissions and rerun the restore.")
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AtmError::file_policy(format!(
                "failed to read task directory entry under {}: {error}",
                tasks_dir.display()
            ))
            .with_source(error)
            .with_recovery("Check task directory permissions and rerun the restore.")
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
        AtmErrorKind::FilePolicy,
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
        ))
        .with_recovery(
            "Inspect the stale restore staging directory, remove it after confirming no restore is running, and rerun `atm teams restore`.",
        ));
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
        .with_source(error)
        .with_recovery("Remove the restore staging directory after confirming the restore completed successfully.")
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
        .with_source(error)
        .with_recovery("Check inbox directory permissions and rerun `atm teams restore`.")
    })?;
    lock::sweep_stale_lock_sentinels(&inboxes_dir).map_err(|error| {
        error.with_recovery("Check inbox directory permissions and rerun `atm teams restore`.")
    })?;

    let inbox_staging_dir = restore_staging_inboxes_dir(team_dir);
    fs::create_dir_all(&inbox_staging_dir).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create inbox restore staging directory {}: {error}",
            inbox_staging_dir.display()
        ))
        .with_source(error)
        .with_recovery("Check inbox staging permissions and rerun `atm teams restore`.")
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
            .with_source(error)
            .with_recovery("Check inbox permissions and backup integrity, then rerun the restore.")
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
            .with_source(error)
            .with_recovery("Check inbox permissions and rerun `atm teams restore`.")
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
        AtmErrorKind::FilePolicy,
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
        ))
        .with_recovery(
            "Remove the stale restore marker after verifying the restored team state.",
        ));
    }

    fs::remove_file(&marker).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to remove restore marker {}: {error}",
            marker.display()
        ))
        .with_source(error)
        .with_recovery("Remove the stale restore marker after verifying the restored team state.")
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};

    use chrono::Utc;
    use serde_json::json;
    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        clear_restore_marker, prepare_restore_workspace, restore_marker_path, restore_staging_dir,
        restore_task_state_from_backup, restore_team,
    };
    use crate::schema::TeamConfig;
    use crate::team_admin::RestoreRequest;

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

    fn write_backup_config(backup_dir: &Path, value: serde_json::Value) {
        write_json(&backup_dir.join("config.json"), &value);
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
        let envelope = crate::schema::MessageEnvelope {
            from: "team-lead".parse().expect("agent"),
            text: text.to_string(),
            timestamp: crate::types::IsoTimestamp::from_datetime(Utc::now()),
            read: false,
            source_team: Some("atm-dev".parse().expect("team")),
            summary: None,
            message_id: None,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        };
        write_text(
            path,
            &format!("{}\n", serde_json::to_string(&envelope).expect("envelope")),
        );
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_env_var_serial<T>(key: &'static str, value: &str, body: impl FnOnce() -> T) -> T {
        let _guard = env_lock().lock().expect("env lock");
        let _env_guard = EnvGuard::set_raw(key, value);
        body()
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_raw(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            set_env_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => set_env_var(self.key, value),
                None => remove_env_var(self.key),
            }
        }
    }

    fn set_env_var<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(key: K, value: V) {
        // SAFETY: restore tests that mutate process environment run under
        // `serial_test` and hold the shared env lock for the full mutation
        // window.
        unsafe { std::env::set_var(key, value) };
    }

    fn remove_env_var<K: AsRef<std::ffi::OsStr>>(key: K) {
        // SAFETY: same serialization guarantee as above.
        unsafe { std::env::remove_var(key) };
    }

    #[test]
    fn prepare_restore_workspace_rejects_preexisting_staging_dir() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        let backup_dir = tempdir.path().join("backup");
        fs::create_dir_all(restore_staging_dir(&team_dir)).expect("staging dir");
        fs::create_dir_all(&backup_dir).expect("backup dir");

        let error = prepare_restore_workspace(&team_dir, &backup_dir).expect_err("staging error");

        assert!(error.is_file_policy());
        assert!(
            error
                .message
                .contains("restore staging directory already exists")
        );
        assert!(!restore_marker_path(&team_dir).exists());
    }

    #[test]
    fn restore_task_state_from_backup_round_trips_highwatermark() {
        let tempdir = tempdir().expect("tempdir");
        let backup_tasks_dir = tempdir.path().join("backup").join("tasks");
        let tasks_dir = tempdir.path().join(".claude").join("tasks").join("atm-dev");
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
    #[serial]
    fn restore_team_keeps_config_last_and_marker_on_config_write_failure() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(
            tempdir.path(),
            "atm-dev",
            json!({"leadSessionId":"lead-current","members":[{"name":"team-lead"}]}),
        );
        let backup_dir = tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(".backups")
            .join("atm-dev")
            .join("20260423T010203000000000Z");
        write_backup_config(
            &backup_dir,
            json!({
                "leadSessionId":"lead-backup",
                "members":[
                    {"name":"team-lead"},
                    {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
                ]
            }),
        );
        write_inbox(
            &backup_dir.join("inboxes").join("arch-ctm.json"),
            "restored worker inbox",
        );
        write_json(
            &backup_dir.join("tasks").join("80.json"),
            &json!({"id":"80"}),
        );

        let result = with_env_var_serial("ATM_TEST_FAIL_TEAM_CONFIG_WRITE", "1", || {
            restore_team(RestoreRequest {
                home_dir: tempdir.path().to_path_buf(),
                team: "atm-dev".parse().expect("team"),
                from: Some(backup_dir.clone()),
                dry_run: false,
            })
        });

        let error = result.expect_err("restore failure");
        assert!(error.is_file_policy());
        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        let config: TeamConfig =
            serde_json::from_slice(&fs::read(team_dir.join("config.json")).expect("config"))
                .expect("parse config");
        assert_eq!(config.members.len(), 1);
        assert_eq!(config.members[0].name, "team-lead");
        assert!(team_dir.join("inboxes").join("arch-ctm.json").is_file());
        assert!(
            tempdir
                .path()
                .join(".claude")
                .join("tasks")
                .join("atm-dev")
                .join("80.json")
                .is_file()
        );
        assert!(restore_marker_path(&team_dir).is_file());
    }

    #[test]
    #[serial]
    fn restore_team_treats_marker_cleanup_failure_as_warning_only() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(
            tempdir.path(),
            "atm-dev",
            json!({"leadSessionId":"lead-current","members":[{"name":"team-lead"}]}),
        );
        let backup_dir = tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(".backups")
            .join("atm-dev")
            .join("20260423T020304000000000Z");
        write_backup_config(
            &backup_dir,
            json!({
                "leadSessionId":"lead-backup",
                "members":[
                    {"name":"team-lead"},
                    {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
                ]
            }),
        );
        write_inbox(
            &backup_dir.join("inboxes").join("arch-ctm.json"),
            "restored worker inbox",
        );

        let result = with_env_var_serial("ATM_TEST_FAIL_RESTORE_MARKER_REMOVE", "1", || {
            restore_team(RestoreRequest {
                home_dir: tempdir.path().to_path_buf(),
                team: "atm-dev".parse().expect("team"),
                from: Some(backup_dir.clone()),
                dry_run: false,
            })
        });

        assert!(
            result.is_ok(),
            "restore should succeed despite marker cleanup"
        );
        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        assert!(restore_marker_path(&team_dir).is_file());
        let config: TeamConfig =
            serde_json::from_slice(&fs::read(team_dir.join("config.json")).expect("config"))
                .expect("parse config");
        assert!(
            config
                .members
                .iter()
                .any(|member| member.name == "arch-ctm")
        );
    }

    #[test]
    #[serial]
    fn restore_team_cleans_staging_and_preserves_live_config_on_inbox_stage_failure() {
        let tempdir = tempdir().expect("tempdir");
        write_team_config(
            tempdir.path(),
            "atm-dev",
            json!({"leadSessionId":"lead-current","members":[{"name":"team-lead"}]}),
        );
        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        let backup_dir = tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(".backups")
            .join("atm-dev")
            .join("20260424T022700000000000Z");
        write_backup_config(
            &backup_dir,
            json!({
                "leadSessionId":"lead-backup",
                "members":[
                    {"name":"team-lead"},
                    {"name":"arch-ctm","agentType":"general-purpose","model":"sonnet","cwd":"/repo"}
                ]
            }),
        );
        write_inbox(
            &backup_dir.join("inboxes").join("arch-ctm.json"),
            "restored worker inbox",
        );

        let result = with_env_var_serial("ATM_TEST_FAIL_RESTORE_INBOX_STAGE", "1", || {
            restore_team(RestoreRequest {
                home_dir: tempdir.path().to_path_buf(),
                team: "atm-dev".parse().expect("team"),
                from: Some(backup_dir.clone()),
                dry_run: false,
            })
        });

        let error = result.expect_err("restore should fail on injected inbox stage error");
        assert!(error.is_mailbox_write());
        assert!(!restore_staging_dir(&team_dir).exists());
        let config: TeamConfig =
            serde_json::from_slice(&fs::read(team_dir.join("config.json")).expect("config"))
                .expect("parse config");
        assert_eq!(config.members.len(), 1);
        assert_eq!(config.members[0].name, "team-lead");
        assert!(!team_dir.join("inboxes").join("arch-ctm.json").exists());
        assert!(restore_marker_path(&team_dir).is_file());
    }

    #[test]
    fn clear_restore_marker_missing_file_is_ok() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        fs::create_dir_all(&team_dir).expect("team dir");

        clear_restore_marker(&team_dir).expect("missing marker should be ok");
    }
}
