use std::env;
use std::path::{Path, PathBuf};

use crate::address::validate_path_segment;
use crate::error::AtmError;
use crate::types::{AgentName, TeamName};

const MAX_HOST_LOG_DIR_UTF8_BYTES: usize = 4096;

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

/// Resolve the host-scoped ATM runtime lock-file path independent of `ATM_HOME`.
///
/// # Errors
///
/// Returns [`AtmError`] when the OS user-home directory cannot be resolved.
pub fn host_runtime_lock_path(file_name: &str) -> Result<PathBuf, AtmError> {
    Ok(host_runtime_lock_path_from_home(
        &resolve_user_home()?,
        file_name,
    ))
}

/// Resolve the host-scoped ATM runtime lock-file path from an explicit user-home root.
pub fn host_runtime_lock_path_from_home(home_dir: &Path, file_name: &str) -> PathBuf {
    host_runtime_dir_from_home(home_dir).join(file_name)
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

/// Resolve the host-scoped ATM retained log directory independent of `ATM_HOME`.
///
/// # Errors
///
/// Returns [`AtmError`] when the OS user-home directory cannot be resolved.
pub fn host_log_dir() -> Result<PathBuf, AtmError> {
    let user_home = resolve_user_home()?;
    if let Some(raw_path) = env::var_os("ATM_LOG_DIR").filter(|value| !value.is_empty()) {
        let raw_path = raw_path.to_str().ok_or_else(|| {
            AtmError::config("ATM_LOG_DIR must be valid UTF-8").with_recovery(
                "Set ATM_LOG_DIR to an absolute UTF-8 local filesystem path outside ~/.claude/ and ~/.atm/daemon/ before retrying.",
            )
        })?;
        if raw_path.len() > MAX_HOST_LOG_DIR_UTF8_BYTES {
            return Err(
                AtmError::config(format!(
                    "ATM_LOG_DIR must not exceed {MAX_HOST_LOG_DIR_UTF8_BYTES} UTF-8 bytes"
                ))
                .with_recovery(
                    "Shorten ATM_LOG_DIR to a local filesystem path no longer than 4096 UTF-8 bytes before retrying.",
                ),
            );
        }
        let path = PathBuf::from(raw_path);
        if !path.is_absolute() {
            return Err(
                AtmError::config(format!(
                    "ATM_LOG_DIR must be an absolute path: {}",
                    path.display()
                ))
                .with_recovery(
                    "Set ATM_LOG_DIR to an absolute local filesystem path outside ~/.claude/ and ~/.atm/daemon/ before retrying.",
                ),
            );
        }
        let daemon_dir = host_runtime_dir_from_home(&user_home);
        if path.starts_with(&daemon_dir) || daemon_dir.starts_with(&path) {
            return Err(
                AtmError::config(format!(
                    "ATM_LOG_DIR must not overlap the host-scoped daemon runtime directory {}",
                    daemon_dir.display()
                ))
                .with_recovery(
                    "Point ATM_LOG_DIR at a separate absolute log directory outside ~/.atm/daemon/ before retrying.",
                ),
            );
        }
        let claude_dir = user_home.join(".claude");
        if path.starts_with(&claude_dir) {
            return Err(
                AtmError::config(format!(
                    "ATM_LOG_DIR must not resolve under the Claude home directory {}",
                    claude_dir.display()
                ))
                .with_recovery(
                    "Point ATM_LOG_DIR at an ATM-owned absolute log directory outside ~/.claude/ before retrying.",
                ),
            );
        }
        return Ok(path);
    }

    Ok(host_log_dir_from_home(&user_home))
}

/// Resolve the host-scoped ATM retained log directory from an explicit user-home root.
pub fn host_log_dir_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".atm").join("logs")
}

/// Resolve the team directory for `team` under the current ATM home.
///
/// # Errors
///
/// Propagates [`atm_home`] failures when the ATM home directory cannot be
/// resolved.
pub fn team_dir(team: &TeamName) -> Result<PathBuf, AtmError> {
    team_dir_from_home(&atm_home()?, team)
}

/// Resolve the primary inbox path for `agent` in `team` under the current ATM home.
///
/// # Errors
///
/// Propagates [`atm_home`] failures when the ATM home directory cannot be
/// resolved.
pub fn inbox_path(team: &TeamName, agent: &AgentName) -> Result<PathBuf, AtmError> {
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
pub fn team_dir_from_home(home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
    validate_path_segment(team.as_str(), "team")?;
    Ok(home_dir.join(".claude").join("teams").join(team.as_str()))
}

/// Resolve the primary inbox path for `agent` in `team` under an explicit ATM home root.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`] when `team` or
/// `agent` contains path traversal, path separators, or other invalid
/// path-segment characters.
pub fn inbox_path_from_home(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<PathBuf, AtmError> {
    validate_path_segment(agent.as_str(), "agent")?;
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
    team: &TeamName,
    agent: &AgentName,
) -> Result<PathBuf, AtmError> {
    validate_path_segment(agent.as_str(), "agent")?;
    Ok(team_dir_from_home(home_dir, team)?
        .join(".atm-state")
        .join("workflow")
        .join(format!("{agent}.json")))
}

pub fn resolve_user_home() -> Result<PathBuf, AtmError> {
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

/// Unix-only ATM_LOG_DIR validation tests cover non-UTF-8 and path-shape cases.
/// Windows keeps these invariants compile-checked here, and cross-target CI verifies the
/// shared `host_log_dir()` contract even though the path-shape override cases below stay Unix-only.
#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use tempfile::TempDir;

    #[cfg(unix)]
    use super::MAX_HOST_LOG_DIR_UTF8_BYTES;
    use super::{
        atm_home, host_db_dir_from_home, host_log_dir_from_home, host_mail_db_path_from_home,
        host_runtime_dir_from_home, host_runtime_lock_path_from_home, inbox_path,
        inbox_path_from_home, team_dir, team_dir_from_home, workflow_state_path_from_home,
    };
    #[cfg(unix)]
    use super::{host_db_dir, host_log_dir, host_mail_db_path, host_runtime_dir};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, TeamName};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct LocalEnvGuard {
        key: &'static str,
        original: Option<OsString>,
        _guard: MutexGuard<'static, ()>,
    }

    impl LocalEnvGuard {
        fn set_raw(key: &'static str, value: &str) -> Self {
            let guard = env_lock().lock().expect("env lock");
            let original = std::env::var_os(key);
            // SAFETY: this helper serializes all environment mutation with the
            // local process-wide mutex it holds for the full guard lifetime.
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                original,
                _guard: guard,
            }
        }

        #[cfg(unix)]
        fn set_many<const N: usize>(changes: [(&'static str, Option<&str>); N]) -> LocalEnvSet {
            let guard = env_lock().lock().expect("env lock");
            let restorations = changes
                .into_iter()
                .map(|(key, value)| {
                    let original = std::env::var_os(key);
                    match value {
                        Some(value) => {
                            // SAFETY: serialized by the local env mutex above.
                            unsafe { std::env::set_var(key, value) };
                        }
                        None => {
                            // SAFETY: serialized by the local env mutex above.
                            unsafe { std::env::remove_var(key) };
                        }
                    }
                    (key, original)
                })
                .collect();
            LocalEnvSet {
                restorations,
                _guard: guard,
            }
        }
    }

    #[cfg(unix)]
    struct LocalEnvSet {
        restorations: Vec<(&'static str, Option<OsString>)>,
        _guard: MutexGuard<'static, ()>,
    }

    impl Drop for LocalEnvGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => {
                    // SAFETY: the guard still holds the local env mutex.
                    unsafe { std::env::set_var(self.key, value) }
                }
                None => {
                    // SAFETY: the guard still holds the local env mutex.
                    unsafe { std::env::remove_var(self.key) }
                }
            }
        }
    }

    #[cfg(unix)]
    impl Drop for LocalEnvSet {
        fn drop(&mut self) {
            for (key, original) in self.restorations.iter_mut().rev() {
                match original.take() {
                    Some(value) => {
                        // SAFETY: the guard still holds the local env mutex.
                        unsafe { std::env::set_var(key, value) }
                    }
                    None => {
                        // SAFETY: the guard still holds the local env mutex.
                        unsafe { std::env::remove_var(key) }
                    }
                }
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn atm_home_prefers_atm_home_env() {
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_home =
            LocalEnvGuard::set_raw("ATM_HOME", tempdir.path().to_str().expect("utf8 path"));

        let resolved = atm_home().expect("atm home");
        assert_eq!(resolved, tempdir.path());
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn atm_home_falls_back_to_home_dir() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = LocalEnvGuard::set_many([
            ("ATM_HOME", None),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
        ]);

        let resolved = atm_home().expect("atm home");
        assert_eq!(resolved, tempdir.path());
    }

    #[test]
    #[serial_test::serial]
    fn team_and_inbox_paths_use_claude_team_layout() {
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_home =
            LocalEnvGuard::set_raw("ATM_HOME", tempdir.path().to_str().expect("utf8 path"));
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let agent: AgentName = TEST_SENDER.parse().expect("agent");

        assert_eq!(
            team_dir(&team).expect("team dir"),
            tempdir.path().join(".claude").join("teams").join(TEST_TEAM)
        );
        assert_eq!(
            inbox_path(&team, &agent).expect("inbox path"),
            tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join(TEST_TEAM)
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json"))
        );
    }

    #[test]
    fn host_runtime_lock_path_uses_host_runtime_root() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = host_runtime_lock_path_from_home(tempdir.path(), "launch.lock");

        assert_eq!(
            path,
            tempdir
                .path()
                .join(".atm")
                .join("daemon")
                .join("launch.lock")
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_runtime_dir_uses_os_home_not_atm_home() {
        let tempdir = TempDir::new().expect("tempdir");
        let atm_home_dir = TempDir::new().expect("atm home tempdir");
        let _env = LocalEnvGuard::set_many([
            (
                "ATM_HOME",
                Some(atm_home_dir.path().to_str().expect("utf8 path")),
            ),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
        ]);

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
        let atm_home_dir = TempDir::new().expect("atm home");
        let os_home_dir = TempDir::new().expect("os home");
        let _env = LocalEnvGuard::set_many([
            (
                "ATM_HOME",
                Some(atm_home_dir.path().to_str().expect("utf8 path")),
            ),
            (
                "HOME",
                Some(os_home_dir.path().to_str().expect("utf8 path")),
            ),
        ]);

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
        let atm_home_dir = TempDir::new().expect("atm home");
        let os_home_dir = TempDir::new().expect("os home");
        let _env = LocalEnvGuard::set_many([
            (
                "ATM_HOME",
                Some(atm_home_dir.path().to_str().expect("utf8 path")),
            ),
            (
                "HOME",
                Some(os_home_dir.path().to_str().expect("utf8 path")),
            ),
        ]);

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
    fn host_log_dir_from_home_uses_fixed_atm_logs_subtree() {
        let tempdir = TempDir::new().expect("tempdir");
        assert_eq!(
            host_log_dir_from_home(tempdir.path()),
            tempdir.path().join(".atm").join("logs")
        );
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_log_dir_prefers_atm_log_dir_override() {
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_log_dir =
            LocalEnvGuard::set_raw("ATM_LOG_DIR", tempdir.path().to_str().expect("utf8 path"));

        let resolved = host_log_dir().expect("host log dir");
        assert_eq!(resolved, tempdir.path());
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_log_dir_uses_os_home_not_atm_home() {
        let atm_home_dir = TempDir::new().expect("atm home");
        let os_home_dir = TempDir::new().expect("os home");
        let _env = LocalEnvGuard::set_many([
            (
                "ATM_HOME",
                Some(atm_home_dir.path().to_str().expect("utf8 path")),
            ),
            ("ATM_LOG_DIR", None),
            (
                "HOME",
                Some(os_home_dir.path().to_str().expect("utf8 path")),
            ),
        ]);

        let resolved = host_log_dir().expect("host log dir");
        assert_eq!(resolved, os_home_dir.path().join(".atm").join("logs"));
    }

    #[test]
    fn team_dir_from_home_rejects_path_traversal_segments() {
        let tempdir = TempDir::new().expect("tempdir");
        let error = "../evil"
            .parse::<TeamName>()
            .and_then(|team| team_dir_from_home(tempdir.path(), &team))
            .expect_err("invalid team");

        assert!(error.is_address());
        assert!(error.message.contains("team name"));
    }

    #[test]
    fn inbox_path_from_home_rejects_path_traversal_segments() {
        let tempdir = TempDir::new().expect("tempdir");
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let error = "../evil"
            .parse::<AgentName>()
            .and_then(|agent| inbox_path_from_home(tempdir.path(), &team, &agent))
            .expect_err("invalid agent");

        assert!(error.is_address());
        assert!(error.message.contains("agent name"));
    }

    #[test]
    fn workflow_state_path_uses_atm_state_layout() {
        let tempdir = TempDir::new().expect("tempdir");
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let agent: AgentName = TEST_SENDER.parse().expect("agent");

        assert_eq!(
            workflow_state_path_from_home(tempdir.path(), &team, &agent)
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

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_log_dir_rejects_non_absolute_override() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = LocalEnvGuard::set_many([
            ("ATM_LOG_DIR", Some("relative/logs")),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
        ]);

        let error = host_log_dir().expect_err("non-absolute ATM_LOG_DIR should fail");
        assert!(error.is_config());
        assert!(error.message.contains("absolute path"));
    }

    /// Windows ATM_LOG_DIR path-shape validation is covered by cross-compile CI
    /// (`cargo xwin check`) rather than native test execution.
    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_log_dir_rejects_claude_subtree_override() {
        let home_dir = TempDir::new().expect("home");
        let forbidden = home_dir.path().join(".claude").join("logs");
        let _env = LocalEnvGuard::set_many([
            ("ATM_LOG_DIR", Some(forbidden.to_str().expect("utf8 path"))),
            ("HOME", Some(home_dir.path().to_str().expect("utf8 path"))),
        ]);

        let error = host_log_dir().expect_err("claude subtree should fail");
        assert!(error.is_config());
        assert!(error.message.contains(".claude"));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_log_dir_rejects_daemon_overlap_override() {
        let home_dir = TempDir::new().expect("home");
        let forbidden = home_dir.path().join(".atm").join("daemon").join("logs");
        let _env = LocalEnvGuard::set_many([
            ("ATM_LOG_DIR", Some(forbidden.to_str().expect("utf8 path"))),
            ("HOME", Some(home_dir.path().to_str().expect("utf8 path"))),
        ]);

        let error = host_log_dir().expect_err("daemon overlap should fail");
        assert!(error.is_config());
        assert!(error.message.contains(".atm/daemon"));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_log_dir_rejects_non_utf8_override() {
        use std::os::unix::ffi::OsStringExt;

        let home_dir = TempDir::new().expect("home");
        let _home = LocalEnvGuard::set_raw("HOME", home_dir.path().to_str().expect("utf8 path"));
        let _atm_log_dir = Utf8EnvGuard {
            key: "ATM_LOG_DIR",
            original: std::env::var_os("ATM_LOG_DIR"),
        };
        // SAFETY: the test helper serializes environment mutation in-process.
        unsafe { std::env::set_var("ATM_LOG_DIR", OsString::from_vec(vec![0x66, 0x6f, 0x80])) };

        let error = host_log_dir().expect_err("non-utf8 override should fail");
        assert!(error.is_config());
        assert!(error.message.contains("UTF-8"));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn host_log_dir_rejects_overlong_override() {
        let home_dir = TempDir::new().expect("home");
        let too_long = format!("/{}", "a".repeat(MAX_HOST_LOG_DIR_UTF8_BYTES));
        let _env = LocalEnvGuard::set_many([
            ("ATM_LOG_DIR", Some(too_long.as_str())),
            ("HOME", Some(home_dir.path().to_str().expect("utf8 path"))),
        ]);

        let error = host_log_dir().expect_err("overlong ATM_LOG_DIR should fail");
        assert!(error.is_config());
        assert!(error.message.contains("4096"));
    }

    #[cfg(unix)]
    struct Utf8EnvGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    #[cfg(unix)]
    impl Drop for Utf8EnvGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => {
                    // SAFETY: the shared test env guard serializes process environment mutation.
                    unsafe { std::env::set_var(self.key, value) }
                }
                None => {
                    // SAFETY: the shared test env guard serializes process environment mutation.
                    unsafe { std::env::remove_var(self.key) }
                }
            }
        }
    }
}
