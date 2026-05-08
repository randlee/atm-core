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

/// Resolve the host-scoped ATM runtime directory independent of `ATM_HOME`.
///
/// # Errors
///
/// Returns [`AtmError`] when the OS user-home directory cannot be resolved.
pub fn host_runtime_dir() -> Result<PathBuf, AtmError> {
    Ok(host_runtime_dir_from_home(&resolve_user_home()?))
}

/// Resolve the host-scoped ATM runtime directory from an explicit user-home root.
pub fn host_runtime_dir_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".atm").join("daemon")
}

/// Resolve the host-scoped ATM durable-state directory independent of `ATM_HOME`.
///
/// # Errors
///
/// Returns [`AtmError`] when the OS user-home directory cannot be resolved.
pub fn host_db_dir() -> Result<PathBuf, AtmError> {
    Ok(host_db_dir_from_home(&resolve_user_home()?))
}

/// Resolve the host-scoped ATM durable-state directory from an explicit user-home root.
pub fn host_db_dir_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".atm").join("db")
}

/// Resolve the host-scoped ATM durable mailbox database path independent of `ATM_HOME`.
///
/// # Errors
///
/// Returns [`AtmError`] when the OS user-home directory cannot be resolved.
pub fn host_mail_db_path() -> Result<PathBuf, AtmError> {
    Ok(host_mail_db_path_from_home(&resolve_user_home()?))
}

/// Resolve the host-scoped ATM durable mailbox database path from an explicit user-home root.
pub fn host_mail_db_path_from_home(home_dir: &Path) -> PathBuf {
    host_db_dir_from_home(home_dir).join("mail.db")
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

    #[cfg(unix)]
    use super::{
        host_db_dir, host_db_dir_from_home, host_mail_db_path, host_mail_db_path_from_home,
        host_runtime_dir, host_runtime_dir_from_home,
    };
    use super::{
        atm_home, inbox_path, inbox_path_from_home, team_dir, team_dir_from_home,
        workflow_state_path_from_home,
    };
    use crate::test_support::{TEST_SENDER, TEST_TEAM};

    // Serializes process-environment mutation inside this test module. This is
    // process-local only; it does not coordinate with other test processes.
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
            team_dir(TEST_TEAM).expect("team dir"),
            tempdir.path().join(".claude").join("teams").join(TEST_TEAM)
        );
        assert_eq!(
            inbox_path(TEST_TEAM, TEST_SENDER).expect("inbox path"),
            tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join(TEST_TEAM)
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json"))
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_runtime_dir_uses_os_home_not_atm_home() {
        let _guard = env_lock().lock().expect("env lock");
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home_dir = TempDir::new().expect("atm home tempdir");
        let _atm_home = EnvGuard::set("ATM_HOME", atm_home_dir.path());
        let _home = EnvGuard::set_raw("HOME", tempdir.path().to_str().expect("utf8 path"));

        let resolved = host_runtime_dir().expect("host runtime dir");
        assert_eq!(resolved, tempdir.path().join(".atm").join("daemon"));
    }

    #[test]
    fn host_runtime_dir_from_home_uses_fixed_atm_daemon_subtree() {
        let tempdir = TempDir::new().expect("tempdir");
        assert_eq!(
            host_runtime_dir_from_home(tempdir.path()),
            tempdir.path().join(".atm").join("daemon")
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_db_dir_uses_os_home_not_atm_home() {
        let _guard = env_lock().lock().expect("env lock");
        let atm_home_dir = TempDir::new().expect("atm home");
        let os_home_dir = TempDir::new().expect("os home");
        let _atm_home = EnvGuard::set("ATM_HOME", atm_home_dir.path());
        let _home = EnvGuard::set_raw("HOME", os_home_dir.path().to_str().expect("utf8 path"));

        let resolved = host_db_dir().expect("host db dir");
        assert_eq!(resolved, os_home_dir.path().join(".atm").join("db"));
    }

    #[test]
    fn host_db_dir_from_home_uses_fixed_atm_db_subtree() {
        let tempdir = TempDir::new().expect("tempdir");
        assert_eq!(
            host_db_dir_from_home(tempdir.path()),
            tempdir.path().join(".atm").join("db")
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_mail_db_path_uses_os_home_not_atm_home() {
        let _guard = env_lock().lock().expect("env lock");
        let atm_home_dir = TempDir::new().expect("atm home");
        let os_home_dir = TempDir::new().expect("os home");
        let _atm_home = EnvGuard::set("ATM_HOME", atm_home_dir.path());
        let _home = EnvGuard::set_raw("HOME", os_home_dir.path().to_str().expect("utf8 path"));

        let resolved = host_mail_db_path().expect("host mail db path");
        assert_eq!(
            resolved,
            os_home_dir.path().join(".atm").join("db").join("mail.db")
        );
    }

    #[test]
    fn host_mail_db_path_from_home_uses_mail_db_filename() {
        let tempdir = TempDir::new().expect("tempdir");
        assert_eq!(
            host_mail_db_path_from_home(tempdir.path()),
            tempdir.path().join(".atm").join("db").join("mail.db")
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
            inbox_path_from_home(tempdir.path(), TEST_TEAM, "../evil").expect_err("invalid agent");

        assert!(error.is_address());
        assert!(error.message.contains("agent name"));
    }

    #[test]
    fn workflow_state_path_uses_atm_state_layout() {
        let tempdir = TempDir::new().expect("tempdir");

        assert_eq!(
            workflow_state_path_from_home(tempdir.path(), TEST_TEAM, TEST_SENDER)
                .expect("workflow state path"),
            tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join(TEST_TEAM)
                .join(".atm-state")
                .join("workflow")
                .join(format!("{TEST_SENDER}.json"))
        );
    }
}
