use std::env;
use std::path::{Path, PathBuf};

use crate::address::validate_path_segment;
use crate::error::AtmError;

/// Resolve the ATM home directory for the current process.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::ConfigHomeUnavailable`] when neither
/// `ATM_HOME` nor the OS user-home environment variables can be resolved.
pub fn atm_home() -> Result<PathBuf, AtmError> {
    if let Some(home) = env::var_os("ATM_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }

    resolve_user_home()
}

/// Resolve the team directory for `team` under the current ATM home.
///
/// # Errors
///
/// Propagates [`atm_home`] failures when the ATM home directory cannot be
/// resolved.
pub fn team_dir(team: &str) -> Result<PathBuf, AtmError> {
    team_dir_from_home(&atm_home()?, team)
}

/// Resolve the primary inbox path for `agent` in `team` under the current ATM home.
///
/// # Errors
///
/// Propagates [`atm_home`] failures when the ATM home directory cannot be
/// resolved.
pub fn inbox_path(team: &str, agent: &str) -> Result<PathBuf, AtmError> {
    inbox_path_from_home(&atm_home()?, team, agent)
}

/// Resolve the team directory for `team` under an explicit ATM home root.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`] when `team`
/// contains path traversal, path separators, or other invalid path-segment
/// characters.
pub fn team_dir_from_home(home_dir: &Path, team: &str) -> Result<PathBuf, AtmError> {
    validate_path_segment(team, "team")?;
    Ok(home_dir.join(".claude").join("teams").join(team))
}

/// Resolve the primary inbox path for `agent` in `team` under an explicit ATM home root.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`] when `team` or
/// `agent` contains path traversal, path separators, or other invalid
/// path-segment characters.
pub fn inbox_path_from_home(home_dir: &Path, team: &str, agent: &str) -> Result<PathBuf, AtmError> {
    validate_path_segment(agent, "agent")?;
    Ok(team_dir_from_home(home_dir, team)?
        .join("inboxes")
        .join(format!("{agent}.json")))
}

/// Resolve the ATM-owned workflow-state path for `agent` in `team`.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`] when `team` or
/// `agent` contains path traversal, path separators, or other invalid
/// path-segment characters.
pub fn workflow_state_path_from_home(
    home_dir: &Path,
    team: &str,
    agent: &str,
) -> Result<PathBuf, AtmError> {
    validate_path_segment(agent, "agent")?;
    Ok(team_dir_from_home(home_dir, team)?
        .join(".atm-state")
        .join("workflow")
        .join(format!("{agent}.json")))
}

fn resolve_user_home() -> Result<PathBuf, AtmError> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .ok_or_else(AtmError::home_directory_unavailable)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    use tempfile::TempDir;

    use super::{
        atm_home, inbox_path, inbox_path_from_home, team_dir, team_dir_from_home,
        workflow_state_path_from_home,
    };

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let original = std::env::var_os(key);
            set_env_var(key, value);
            Self { key, original }
        }

        #[cfg(unix)]
        fn set_raw(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            set_env_var(key, value);
            Self { key, original }
        }

        #[cfg(unix)]
        fn remove(key: &'static str) -> Self {
            let original = std::env::var_os(key);
            remove_env_var(key);
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
        // SAFETY: these tests take a process-wide mutex before mutating the
        // environment, so the mutation is serialized within this process.
        unsafe { std::env::set_var(key, value) }
    }

    fn remove_env_var<K: AsRef<std::ffi::OsStr>>(key: K) {
        // SAFETY: these tests take a process-wide mutex before mutating the
        // environment, so the mutation is serialized within this process.
        unsafe { std::env::remove_var(key) }
    }

    #[test]
    #[serial_test::serial]
    fn atm_home_prefers_atm_home_env() {
        let _guard = env_lock().lock().expect("env lock");
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_home = EnvGuard::set("ATM_HOME", tempdir.path());

        let resolved = atm_home().expect("atm home");
        assert_eq!(resolved, tempdir.path());
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn atm_home_falls_back_to_home_dir() {
        let _guard = env_lock().lock().expect("env lock");
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_home = EnvGuard::remove("ATM_HOME");
        let _home = EnvGuard::set_raw("HOME", tempdir.path().to_str().expect("utf8 path"));

        let resolved = atm_home().expect("atm home");
        assert_eq!(resolved, tempdir.path());
    }

    #[test]
    #[serial_test::serial]
    fn team_and_inbox_paths_use_claude_team_layout() {
        let _guard = env_lock().lock().expect("env lock");
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_home = EnvGuard::set("ATM_HOME", tempdir.path());

        assert_eq!(
            team_dir("atm-dev").expect("team dir"),
            tempdir.path().join(".claude").join("teams").join("atm-dev")
        );
        assert_eq!(
            inbox_path("atm-dev", "arch-ctm").expect("inbox path"),
            tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join("atm-dev")
                .join("inboxes")
                .join("arch-ctm.json")
        );
    }

    #[test]
    fn team_dir_from_home_rejects_path_traversal_segments() {
        let tempdir = TempDir::new().expect("tempdir");
        let error = team_dir_from_home(tempdir.path(), "../evil").expect_err("invalid team");

        assert!(error.is_address());
        assert!(error.message.contains("team name"));
    }

    #[test]
    fn inbox_path_from_home_rejects_path_traversal_segments() {
        let tempdir = TempDir::new().expect("tempdir");
        let error =
            inbox_path_from_home(tempdir.path(), "atm-dev", "../evil").expect_err("invalid agent");

        assert!(error.is_address());
        assert!(error.message.contains("agent name"));
    }

    #[test]
    fn workflow_state_path_uses_atm_state_layout() {
        let tempdir = TempDir::new().expect("tempdir");

        assert_eq!(
            workflow_state_path_from_home(tempdir.path(), "atm-dev", "arch-ctm")
                .expect("workflow state path"),
            tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join("atm-dev")
                .join(".atm-state")
                .join("workflow")
                .join("arch-ctm.json")
        );
    }
}
