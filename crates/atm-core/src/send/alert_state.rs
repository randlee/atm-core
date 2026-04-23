use std::collections::BTreeSet;
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::error::{AtmError, AtmErrorCode, AtmErrorKind};
use crate::persistence;
use crate::process::process_is_alive;

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct SendAlertState {
    #[serde(default)]
    pub(super) missing_team_config_keys: BTreeSet<String>,
}

/// Owner-layer state path for ATM-owned send alert coordination state.
pub(super) fn state_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".config").join("atm").join("state.json")
}

pub(super) fn lock_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".config").join("atm").join("state.lock")
}

pub(super) fn load(path: &Path) -> Result<SendAlertState, AtmError> {
    if !path.exists() {
        return Ok(SendAlertState::default());
    }

    let raw = fs::read_to_string(path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "failed to read send alert state at {}: {error}",
                path.display()
            ),
        )
        .with_recovery("Check ATM config-state permissions or remove the damaged state file before retrying the send command.")
        .with_source(error)
    })?;
    serde_json::from_str(&raw).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "failed to parse send alert state at {}: {error}",
                path.display()
            ),
        )
        .with_recovery(
            "Remove the malformed send alert state file so ATM can recreate it on the next send.",
        )
        .with_source(error)
    })
}

/// Persist ATM-owned send alert coordination state through one owner helper.
pub(super) fn save(path: &Path, state: &SendAlertState) -> Result<(), AtmError> {
    let data = serde_json::to_vec(state)?;
    persistence::atomic_write_bytes(
        path,
        &data,
        AtmErrorKind::Config,
        "send alert state",
        "Check ATM config-state directory permissions and rerun the send operation.",
    )
}

pub(super) fn acquire_lock(path: &Path) -> Option<SendAlertLock> {
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        warn!(
            code = %AtmErrorCode::WarningSendAlertStateDegraded,
            %error,
            path = %parent.display(),
            "failed to create send alert lock directory"
        );
        return None;
    }

    for _ in 0..100 {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                let pid = std::process::id().to_string();
                if let Err(error) = std::io::Write::write_all(&mut file, pid.as_bytes()) {
                    warn!(
                        code = %AtmErrorCode::WarningSendAlertStateDegraded,
                        %error,
                        path = %path.display(),
                        "failed to write send alert lock pid"
                    );
                    let _ = fs::remove_file(path);
                    return None;
                }
                return Some(SendAlertLock {
                    path: path.to_path_buf(),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if evict_stale_send_alert_lock(path) {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                warn!(
                    code = %AtmErrorCode::WarningSendAlertStateDegraded,
                    %error,
                    path = %path.display(),
                    "failed to create send alert lock"
                );
                return None;
            }
        }
    }

    None
}

pub(super) struct SendAlertLock {
    path: PathBuf,
}

impl Drop for SendAlertLock {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            warn!(
                code = %AtmErrorCode::WarningSendAlertStateDegraded,
                %error,
                path = %self.path.display(),
                "failed to remove send alert lock"
            );
        }
    }
}

fn evict_stale_send_alert_lock(path: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        return false;
    };
    if process_is_alive(pid) {
        return false;
    }

    match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            warn!(
                code = %AtmErrorCode::WarningSendAlertStateDegraded,
                %error,
                path = %path.display(),
                pid,
                "failed to evict stale send alert lock"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{SendAlertState, acquire_lock, load, lock_path, save, state_path};

    #[test]
    fn load_send_alert_state_missing_file_returns_default() {
        let tempdir = tempdir().expect("tempdir");
        let path = state_path(tempdir.path());

        let state = load(&path).expect("default state");

        assert!(state.missing_team_config_keys.is_empty());
    }

    #[test]
    fn load_send_alert_state_defaults_missing_keys_field() {
        let tempdir = tempdir().expect("tempdir");
        let path = state_path(tempdir.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("state dir");
        }
        fs::write(&path, "{}").expect("state file");

        let state = load(&path).expect("compat state");

        assert!(state.missing_team_config_keys.is_empty());
    }

    #[test]
    fn load_send_alert_state_read_errors_are_config_errors() {
        let tempdir = tempdir().expect("tempdir");
        let path = state_path(tempdir.path());
        fs::create_dir_all(&path).expect("directory instead of file");

        let error = load(&path).expect_err("read error");

        assert!(error.is_config());
        assert!(error.message.contains("failed to read send alert state"));
    }

    #[test]
    fn save_send_alert_state_writes_expected_json_shape() {
        let tempdir = tempdir().expect("tempdir");
        let path = state_path(tempdir.path());
        let mut state = SendAlertState::default();
        state
            .missing_team_config_keys
            .insert("teams/zeta/config.json".to_string());
        state
            .missing_team_config_keys
            .insert("teams/alpha/config.json".to_string());

        save(&path, &state).expect("save");

        let raw = fs::read_to_string(&path).expect("saved state");
        assert_eq!(
            raw,
            "{\"missing_team_config_keys\":[\"teams/alpha/config.json\",\"teams/zeta/config.json\"]}"
        );
    }

    #[test]
    fn acquire_send_alert_lock_creates_parent_writes_pid_and_cleans_up_on_drop() {
        let tempdir = tempdir().expect("tempdir");
        let path = lock_path(tempdir.path());

        let guard = acquire_lock(&path).expect("lock guard");

        assert!(
            path.parent().expect("lock parent").exists(),
            "lock parent directory should be created"
        );
        assert_eq!(
            fs::read_to_string(&path).expect("lock contents").trim(),
            std::process::id().to_string()
        );
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn acquire_send_alert_lock_returns_none_while_live_pid_lock_exists() {
        let tempdir = tempdir().expect("tempdir");
        let path = lock_path(tempdir.path());

        let guard = acquire_lock(&path).expect("first lock");
        let initial_contents = fs::read_to_string(&path).expect("initial lock contents");

        assert!(acquire_lock(&path).is_none());
        assert_eq!(
            fs::read_to_string(&path).expect("lock contents after second attempt"),
            initial_contents
        );

        drop(guard);
        assert!(!path.exists());
    }
    #[test]
    fn acquire_send_alert_lock_evicts_stale_pid_lock_and_reacquires() {
        let tempdir = tempdir().expect("tempdir");
        let path = lock_path(tempdir.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("lock dir");
        }
        fs::write(&path, u32::MAX.to_string()).expect("stale pid lock");

        let guard = acquire_lock(&path).expect("reacquired lock");

        assert_eq!(
            fs::read_to_string(&path)
                .expect("lock contents after eviction")
                .trim(),
            std::process::id().to_string()
        );

        drop(guard);
        assert!(!path.exists());
    }
}
