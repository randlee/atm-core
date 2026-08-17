use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::json;
use tracing::warn;

use super::BackupOutcome;
use crate::address::validate_path_segment;
use crate::boundary::RosterStore;
use crate::error::{AtmError, AtmErrorCode};
use crate::persistence;

pub(super) fn backup_team_from_roster_store(
    roster_store: &dyn RosterStore,
    request: super::BackupRequest,
) -> Result<BackupOutcome, AtmError> {
    let team_dir = crate::home::team_dir_from_home(&request.home_dir, &request.team)?;

    let backup_dir = backup_root_from_home(&request.home_dir, &request.team)?.join(timestamp_dir());
    fs::create_dir_all(backup_dir.join("inboxes")).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to create backup directory {}: {error}",
            backup_dir.display()
        ))
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

pub(super) fn ensure_inbox_exists(inbox_path: &Path) -> Result<bool, AtmError> {
    if inbox_path.exists() {
        return Ok(false);
    }

    if let Some(parent) = inbox_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to create inbox directory {}: {error}",
                parent.display()
            ))
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
        })?;
    Ok(true)
}

pub(super) fn backup_root_from_home(home_dir: &Path, team: &str) -> Result<PathBuf, AtmError> {
    validate_path_segment(team, "team")?;
    Ok(teams_root_from_home(home_dir).join(".backups").join(team))
}

pub(super) fn tasks_dir_from_home(home_dir: &Path, team: &str) -> Result<PathBuf, AtmError> {
    validate_path_segment(team, "team")?;
    Ok(home_dir.join(".claude").join("tasks").join(team))
}

pub(super) fn copy_regular_files_strict<F>(
    src: &Path,
    dst: &Path,
    include: F,
) -> Result<(), AtmError>
where
    F: Fn(&str) -> bool,
{
    copy_regular_files_with_policy(src, dst, include, DirEntryErrorPolicy::FailClosed)
}

fn teams_root_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".claude").join("teams")
}

fn timestamp_dir() -> String {
    let now = Utc::now();
    format!(
        "{}{:09}Z",
        now.format("%Y%m%dT%H%M%S"),
        now.timestamp_subsec_nanos()
    )
}

fn write_roster_audit_snapshot(
    backup_dir: &Path,
    roster_store: &dyn RosterStore,
    team: &crate::types::TeamName,
) -> Result<(), AtmError> {
    let roster = super::projection::load_team_roster(roster_store, team)?;
    let bytes = serde_json::to_vec_pretty(&json!({
        "team": team,
        "members": roster,
    }))
    .map_err(AtmError::from)?;
    persistence::atomic_write_bytes(
        &backup_dir.join("atm-roster.json"),
        &bytes,
        AtmErrorCode::FilePolicyRejected,
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
    })?;

    let mut entries = Vec::new();
    for entry in fs::read_dir(src).map_err(|error| {
        AtmError::file_policy(format!(
            "failed to read source directory {}: {error}",
            src.display()
        ))
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
                    )));
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
        })?;
    }

    Ok(())
}
