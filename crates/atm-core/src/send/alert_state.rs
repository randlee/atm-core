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
