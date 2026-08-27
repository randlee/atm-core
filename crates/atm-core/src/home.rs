use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::address::validate_path_segment;
use crate::error::AtmError;
use crate::types::{AgentName, TeamName};

const MAX_ATM_HOME_UTF8_BYTES: usize = 4096;
const MAX_HOST_LOG_DIR_UTF8_BYTES: usize = 4096;
pub const HOST_RUNTIME_LAUNCH_LOCK_FILE: &str = "launch.lock";
pub const HOST_RUNTIME_OWNER_LOCK_FILE: &str = "owner.lock";
pub const HOST_RUNTIME_SOCKET_FILE: &str = "atm-daemon.sock";

/// OS-user-owned root for singleton admission artifacts.  This wrapper is
/// intentionally not dereferenceable: admission paths must originate in
/// [`current_host_runtime_scope`], not from caller-selected workspace paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeRoot(PathBuf);

impl AsRef<Path> for HostRuntimeRoot {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// OS-user-owned root for the one durable ATM SQLite state store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableStateRoot(PathBuf);

impl AsRef<Path> for DurableStateRoot {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// The sole production source of daemon ownership, endpoint, and durable
/// state paths. `ATM_HOME` remains workspace/config discovery only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeScope {
    pub runtime_root: HostRuntimeRoot,
    pub durable_state_root: DurableStateRoot,
    pub launch_lock: PathBuf,
    pub owner_lock: PathBuf,
    pub socket: PathBuf,
}

pub fn current_host_runtime_scope() -> Result<HostRuntimeScope, AtmError> {
    // This intentionally does not use HOME, USERPROFILE, ATM_HOME, or the
    // current directory: those are process-scoped inputs and therefore cannot
    // define a host-wide singleton boundary.
    let root = os_account_home()?.join(".atm");
    let runtime_root = HostRuntimeRoot(root.join("daemon"));
    let durable_state_root = DurableStateRoot(root.join("db"));
    Ok(HostRuntimeScope {
        launch_lock: runtime_root.as_ref().join(HOST_RUNTIME_LAUNCH_LOCK_FILE),
        owner_lock: runtime_root.as_ref().join(HOST_RUNTIME_OWNER_LOCK_FILE),
        socket: runtime_root.as_ref().join(HOST_RUNTIME_SOCKET_FILE),
        runtime_root,
        durable_state_root,
    })
}

/// Resolve the ATM home directory for the current process.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::ConfigHomeUnavailable`] when neither
/// `ATM_HOME` nor the OS user-home environment variables can be resolved, or a
/// config-shaped [`AtmError`] when the `ATM_HOME` override is non-UTF-8,
/// overlong, or not absolute.
pub fn atm_home() -> Result<PathBuf, AtmError> {
    if let Some(home) = env::var_os("ATM_HOME").filter(|value| !value.is_empty()) {
        return validate_atm_home_os(home.as_os_str());
    }

    validate_atm_home_path(resolve_user_home()?)
}

/// Resolve the current OS user home directory without consulting `ATM_HOME`.
///
/// # Errors
///
/// Returns [`AtmError`] when the OS user-home environment variables cannot be
/// resolved.
pub fn user_home() -> Result<PathBuf, AtmError> {
    resolve_user_home()
}

/// Resolve the invocation directory for the active ATM command process.
///
/// # Errors
///
/// Returns [`AtmError`] when the process working directory cannot be resolved.
pub fn command_invocation_dir() -> Result<PathBuf, AtmError> {
    env::current_dir().map_err(|source| {
        AtmError::runtime_root_invalid("failed to resolve the ATM command invocation directory")
            .with_cause(source)
    })
}

/// Resolve the host-scoped ATM runtime directory from the accepted ATM home.
///
/// # Errors
///
/// Returns [`AtmError`] when the accepted ATM home directory cannot be resolved.
pub fn host_runtime_dir() -> Result<PathBuf, AtmError> {
    Ok(current_host_runtime_scope()?.runtime_root.0)
}

/// Resolve the host-scoped ATM runtime directory from an explicit ATM home root.
pub fn host_runtime_dir_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".atm").join("daemon")
}

/// Resolve the host-scoped ATM runtime lock-file path from the accepted ATM home.
///
/// # Errors
///
/// Returns [`AtmError`] when the accepted ATM home directory cannot be resolved.
pub fn host_runtime_lock_path(file_name: &str) -> Result<PathBuf, AtmError> {
    Ok(current_host_runtime_scope()?
        .runtime_root
        .as_ref()
        .join(file_name))
}

/// Resolve the host-scoped ATM runtime lock-file path from an explicit ATM home root.
pub fn host_runtime_lock_path_from_home(home_dir: &Path, file_name: &str) -> PathBuf {
    host_runtime_dir_from_home(home_dir).join(file_name)
}

/// Resolve the host-scoped ATM durable-state directory from the accepted ATM home.
///
/// # Errors
///
/// Returns [`AtmError`] when the accepted ATM home directory cannot be resolved.
pub fn host_db_dir() -> Result<PathBuf, AtmError> {
    Ok(current_host_runtime_scope()?.durable_state_root.0)
}

/// Resolve the host-scoped ATM durable-state directory from an explicit ATM home root.
pub fn host_db_dir_from_home(home_dir: &Path) -> PathBuf {
    home_dir.join(".atm").join("db")
}

/// Resolve the host-scoped ATM durable mailbox database path from the accepted ATM home.
///
/// # Errors
///
/// Returns [`AtmError`] when the accepted ATM home directory cannot be resolved.
pub fn host_mail_db_path() -> Result<PathBuf, AtmError> {
    Ok(current_host_runtime_scope()?
        .durable_state_root
        .as_ref()
        .join("mail.db"))
}

/// Resolve the host-scoped ATM durable mailbox database path from an explicit ATM home root.
pub fn host_mail_db_path_from_home(home_dir: &Path) -> PathBuf {
    host_db_dir_from_home(home_dir).join("mail.db")
}

/// Resolve the host-scoped ATM retained log directory from the accepted ATM home.
///
/// # Errors
///
/// Returns [`AtmError`] when the accepted ATM home directory cannot be resolved.
pub fn host_log_dir() -> Result<PathBuf, AtmError> {
    if let Some(raw_path) = env::var_os("ATM_LOG_DIR").filter(|value| !value.is_empty()) {
        let raw_path = raw_path
            .to_str()
            .ok_or_else(|| AtmError::config("ATM_LOG_DIR must be valid UTF-8"))?;
        if raw_path.len() > MAX_HOST_LOG_DIR_UTF8_BYTES {
            return Err(AtmError::config(format!(
                "ATM_LOG_DIR must not exceed {MAX_HOST_LOG_DIR_UTF8_BYTES} UTF-8 bytes"
            )));
        }
        let path = PathBuf::from(raw_path);
        if !path.is_absolute() {
            return Err(AtmError::config(format!(
                "ATM_LOG_DIR must be an absolute path: {}",
                path.display()
            )));
        }
        return Ok(path);
    }

    // Retained logs are host-owned observability state, not workspace state.
    Ok(user_home()?.join(".atm").join("logs"))
}

/// Resolve the host-scoped ATM retained log directory from an explicit ATM home root.
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

pub fn resolve_user_home() -> Result<PathBuf, AtmError> {
    resolve_user_home_via(&crate::atm_temp::ProcessEnvSource)
        .ok_or_else(AtmError::home_directory_unavailable)
}

/// The `$HOME`-then-`%USERPROFILE%` precedence [`resolve_user_home`]
/// applies, read through the [`crate::atm_temp::EnvSource`] seam instead of
/// `std::env` directly, so [`crate::transfer_script`]'s home-directory
/// resolution (which needs the same precedence, testable with a fake
/// `EnvSource`) delegates to this one function rather than reimplementing
/// it (RBQA-F001: two independently-invented copies of the same
/// precedence decision).
///
/// Pure precedence logic with no error-type opinion: returns `None`, not a
/// `Result`, so each caller maps that to its own error type
/// (`resolve_user_home` -> [`AtmError`]; `transfer_script::home_dir_from_env`
/// -> `AtmTempError`).
///
/// Note this is a strictly Unicode-valid-only precedence check
/// (`EnvSource::var` returns `Option<String>`, not `OsString`): a `$HOME`
/// set to non-UTF-8 bytes is treated as unset and falls through to
/// `%USERPROFILE%`, unlike the previous direct-`env::var_os` behavior this
/// replaces. This is an accepted, deliberate narrowing that comes with
/// reusing the `EnvSource` seam, which is `String`-typed by design.
#[must_use]
pub fn resolve_user_home_via(env: &dyn crate::atm_temp::EnvSource) -> Option<PathBuf> {
    env.var("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| env.var("USERPROFILE").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

/// Resolve the profile home for the operating-system account that owns this
/// process. Unlike [`resolve_user_home`], this never consults shell environment
/// variables; it is reserved for host-wide runtime ownership and (on
/// Windows) `crate::transfer_script`'s minimum-bar "is this path under the
/// current user's profile" safety check.
#[cfg(unix)]
pub(crate) fn os_account_home() -> Result<PathBuf, AtmError> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    // SAFETY: `geteuid` has no preconditions. `getpwuid` returns either null or
    // a pointer managed by libc whose `pw_dir` is valid until the next passwd
    // lookup in this thread; copy it before returning.
    let passwd = unsafe { libc::getpwuid(libc::geteuid()) };
    if passwd.is_null() {
        return Err(AtmError::home_directory_unavailable());
    }
    // SAFETY: `passwd` was checked for null and `pw_dir` is a NUL-terminated
    // C string supplied by libc for this account record.
    let directory = unsafe { CStr::from_ptr((*passwd).pw_dir) };
    if directory.to_bytes().is_empty() {
        return Err(AtmError::home_directory_unavailable());
    }
    Ok(PathBuf::from(OsStr::from_bytes(directory.to_bytes())))
}

/// Resolve the Windows profile directory through the known-folder API rather
/// than USERPROFILE, which a caller can redirect per process.
#[cfg(windows)]
pub(crate) fn os_account_home() -> Result<PathBuf, AtmError> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Profile, SHGetKnownFolderPath};

    let mut profile = std::ptr::null_mut();
    // SAFETY: the API initializes `profile` on success; it is released with
    // CoTaskMemFree below as required by SHGetKnownFolderPath.
    let status = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_Profile,
            0,
            std::ptr::null_mut::<core::ffi::c_void>() as HANDLE,
            &mut profile,
        )
    };
    if status < 0 || profile.is_null() {
        return Err(AtmError::home_directory_unavailable());
    }
    // SAFETY: `profile` is a null-terminated buffer allocated by the API.
    let mut length = 0;
    unsafe {
        while *profile.add(length) != 0 {
            length += 1;
        }
    }
    // SAFETY: the range was measured up to the terminating null above.
    let path =
        unsafe { std::ffi::OsString::from_wide(std::slice::from_raw_parts(profile, length)) };
    // SAFETY: SHGetKnownFolderPath documents CoTaskMemFree ownership.
    unsafe { CoTaskMemFree(profile.cast()) };
    Ok(PathBuf::from(path))
}

fn validate_atm_home_os(raw_path: &OsStr) -> Result<PathBuf, AtmError> {
    let raw_path = raw_path
        .to_str()
        .ok_or_else(|| AtmError::atm_home_unresolved("ATM_HOME must be valid UTF-8"))?;
    if raw_path.len() > MAX_ATM_HOME_UTF8_BYTES {
        return Err(AtmError::atm_home_unresolved(format!(
            "ATM_HOME must not exceed {MAX_ATM_HOME_UTF8_BYTES} UTF-8 bytes"
        )));
    }
    validate_atm_home_path(PathBuf::from(raw_path))
}

fn validate_atm_home_path(path: PathBuf) -> Result<PathBuf, AtmError> {
    let utf8_path = path
        .to_str()
        .ok_or_else(|| AtmError::atm_home_unresolved("ATM home path must be valid UTF-8"))?;
    if utf8_path.len() > MAX_ATM_HOME_UTF8_BYTES {
        return Err(AtmError::atm_home_unresolved(format!(
            "ATM home path must not exceed {MAX_ATM_HOME_UTF8_BYTES} UTF-8 bytes"
        )));
    }
    if !path.is_absolute() {
        return Err(AtmError::atm_home_unresolved(format!(
            "ATM home path must be an absolute path: {}",
            path.display()
        )));
    }
    Ok(path)
}

/// Unix-only ATM_LOG_DIR validation tests cover non-UTF-8 and path-shape cases.
/// Windows keeps these invariants compile-checked here, and cross-target CI verifies the
/// shared `host_log_dir()` contract even though the path-shape override cases below stay Unix-only.
#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use tempfile::TempDir;

    use super::MAX_ATM_HOME_UTF8_BYTES;
    #[cfg(unix)]
    use super::MAX_HOST_LOG_DIR_UTF8_BYTES;
    #[cfg(unix)]
    use super::os_account_home;
    use super::{
        atm_home, command_invocation_dir, host_db_dir_from_home, host_log_dir,
        host_log_dir_from_home, host_mail_db_path_from_home, host_runtime_dir_from_home,
        host_runtime_lock_path_from_home, inbox_path, inbox_path_from_home, team_dir,
        team_dir_from_home,
    };
    #[cfg(unix)]
    use super::{host_db_dir, host_mail_db_path, host_runtime_dir};
    use crate::test_support::{
        EnvLockGuard, TEST_SENDER, TEST_TEAM, lock_env, remove_env_var, set_env_var,
    };
    use crate::types::{AgentName, TeamName};

    struct LocalEnvGuard {
        key: &'static str,
        original: Option<OsString>,
        _guard: EnvLockGuard,
    }

    impl LocalEnvGuard {
        fn set_raw(key: &'static str, value: &str) -> Self {
            let guard = lock_env();
            let original = std::env::var_os(key);
            set_env_var(key, value);
            Self {
                key,
                original,
                _guard: guard,
            }
        }

        #[cfg(unix)]
        fn set_many<const N: usize>(changes: [(&'static str, Option<&str>); N]) -> LocalEnvSet {
            let guard = lock_env();
            let restorations = changes
                .into_iter()
                .map(|(key, value)| {
                    let original = std::env::var_os(key);
                    match value {
                        Some(value) => set_env_var(key, value),
                        None => remove_env_var(key),
                    }
                    (key, original)
                })
                .collect();
            LocalEnvSet {
                restorations,
                _guard: guard,
            }
        }

        #[cfg(unix)]
        fn set_many_os<const N: usize>(
            changes: [(&'static str, Option<OsString>); N],
        ) -> LocalEnvSet {
            let guard = lock_env();
            let restorations = changes
                .into_iter()
                .map(|(key, value)| {
                    let original = std::env::var_os(key);
                    match value {
                        Some(value) => set_env_var(key, value),
                        None => remove_env_var(key),
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
        _guard: EnvLockGuard,
    }

    impl Drop for LocalEnvGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => set_env_var(self.key, value),
                None => remove_env_var(self.key),
            }
        }
    }

    #[cfg(unix)]
    impl Drop for LocalEnvSet {
        fn drop(&mut self) {
            for (key, original) in self.restorations.iter_mut().rev() {
                match original.take() {
                    Some(value) => set_env_var(key, value),
                    None => remove_env_var(key),
                }
            }
        }
    }

    #[test]
    #[serial_test::serial(env)]
    fn atm_home_prefers_atm_home_env() {
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_home =
            LocalEnvGuard::set_raw("ATM_HOME", tempdir.path().to_str().expect("utf8 path"));

        let resolved = atm_home().expect("atm home");
        assert_eq!(resolved, tempdir.path());
    }

    #[test]
    #[serial_test::serial(env)]
    fn atm_home_rejects_relative_atm_home_override() {
        let _env = LocalEnvGuard::set_raw("ATM_HOME", "relative/home");

        let error = atm_home().expect_err("relative ATM_HOME should fail");

        assert_eq!(
            error.code(),
            crate::error_codes::AtmErrorCode::AtmHomeUnresolved
        );
        assert!(error.message().contains("absolute path"));
    }

    #[test]
    #[serial_test::serial(env)]
    fn atm_home_rejects_overlong_atm_home_override() {
        let tempdir = TempDir::new().expect("tempdir");
        let overlong = tempdir.path().join("a".repeat(MAX_ATM_HOME_UTF8_BYTES));
        let _env = LocalEnvGuard::set_raw("ATM_HOME", overlong.to_str().expect("utf8 path"));

        let error = atm_home().expect_err("overlong ATM_HOME should fail");

        assert_eq!(
            error.code(),
            crate::error_codes::AtmErrorCode::AtmHomeUnresolved
        );
        assert!(error.message().contains("must not exceed"));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
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
    #[serial_test::serial(env)]
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
    #[serial_test::serial(env)]
    fn host_runtime_dir_ignores_atm_home() {
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
        assert_eq!(
            resolved,
            os_account_home()
                .expect("account home")
                .join(".atm")
                .join("daemon")
        );
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
    #[serial_test::serial(env)]
    fn host_db_dir_ignores_atm_home() {
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
        assert_eq!(
            resolved,
            os_account_home()
                .expect("account home")
                .join(".atm")
                .join("db")
        );
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
    #[serial_test::serial(env)]
    fn host_mail_db_path_ignores_atm_home() {
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
            os_account_home()
                .expect("account home")
                .join(".atm")
                .join("db")
                .join("mail.db")
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

    #[test]
    #[serial_test::serial(env)]
    fn host_log_dir_prefers_atm_log_dir_override() {
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_log_dir =
            LocalEnvGuard::set_raw("ATM_LOG_DIR", tempdir.path().to_str().expect("utf8 path"));

        let resolved = host_log_dir().expect("host log dir");
        assert_eq!(resolved, tempdir.path());
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn host_log_dir_override_succeeds_without_home_env() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = LocalEnvGuard::set_many([
            (
                "ATM_LOG_DIR",
                Some(tempdir.path().to_str().expect("utf8 path")),
            ),
            ("HOME", None),
            ("USERPROFILE", None),
        ]);

        let resolved = host_log_dir().expect("host log dir");
        assert_eq!(resolved, tempdir.path());
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn host_log_dir_ignores_atm_home() {
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
    fn command_invocation_dir_matches_process_working_directory() {
        let expected = std::env::current_dir().expect("current dir");
        let resolved = command_invocation_dir().expect("command invocation dir");

        assert_eq!(resolved, expected);
    }

    #[test]
    fn team_dir_from_home_rejects_path_traversal_segments() {
        let tempdir = TempDir::new().expect("tempdir");
        let error = "../evil"
            .parse::<TeamName>()
            .and_then(|team| team_dir_from_home(tempdir.path(), &team))
            .expect_err("invalid team");

        assert!(error.code() == crate::error_codes::AtmErrorCode::AddressParseFailed);
        assert!(error.message().contains("team name"));
    }

    #[test]
    fn inbox_path_from_home_rejects_path_traversal_segments() {
        let tempdir = TempDir::new().expect("tempdir");
        let team: TeamName = TEST_TEAM.parse().expect("team");
        let error = "../evil"
            .parse::<AgentName>()
            .and_then(|agent| inbox_path_from_home(tempdir.path(), &team, &agent))
            .expect_err("invalid agent");

        assert!(error.code() == crate::error_codes::AtmErrorCode::AddressParseFailed);
        assert!(error.message().contains("agent name"));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn host_log_dir_rejects_non_absolute_override() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = LocalEnvGuard::set_many([
            ("ATM_LOG_DIR", Some("relative/logs")),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
        ]);

        let error = host_log_dir().expect_err("non-absolute ATM_LOG_DIR should fail");
        assert!(matches!(
            error.code(),
            crate::error_codes::AtmErrorCode::ConfigHomeUnavailable
                | crate::error_codes::AtmErrorCode::ConfigParseFailed
                | crate::error_codes::AtmErrorCode::ConfigRetiredHookMembersKey
                | crate::error_codes::AtmErrorCode::ConfigRetiredLegacyHookKeys
                | crate::error_codes::AtmErrorCode::ConfigTeamParseFailed
                | crate::error_codes::AtmErrorCode::ConfigTeamMissing
        ));
        assert!(error.message().contains("absolute path"));
    }

    /// Windows ATM_LOG_DIR path-shape validation is covered by cross-compile CI
    /// (`cargo xwin check`) rather than native test execution.
    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn host_log_dir_override_does_not_require_home_relative_claude_validation() {
        let home_dir = TempDir::new().expect("home");
        let override_dir = home_dir.path().join(".claude").join("logs");
        let _env = LocalEnvGuard::set_many([
            (
                "ATM_LOG_DIR",
                Some(override_dir.to_str().expect("utf8 path")),
            ),
            ("HOME", Some(home_dir.path().to_str().expect("utf8 path"))),
        ]);

        let resolved = host_log_dir().expect("claude-relative override should short-circuit");
        assert_eq!(resolved, override_dir);
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn host_log_dir_override_does_not_require_home_relative_daemon_overlap_validation() {
        let home_dir = TempDir::new().expect("home");
        let override_dir = home_dir.path().join(".atm").join("daemon").join("logs");
        let _env = LocalEnvGuard::set_many([
            (
                "ATM_LOG_DIR",
                Some(override_dir.to_str().expect("utf8 path")),
            ),
            ("HOME", Some(home_dir.path().to_str().expect("utf8 path"))),
        ]);

        let resolved = host_log_dir().expect("daemon-overlap override should short-circuit");
        assert_eq!(resolved, override_dir);
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn host_log_dir_rejects_non_utf8_override() {
        use std::os::unix::ffi::OsStringExt;

        let home_dir = TempDir::new().expect("home");
        let _env = LocalEnvGuard::set_many_os([
            (
                "HOME",
                Some(OsString::from(home_dir.path().to_str().expect("utf8 path"))),
            ),
            (
                "ATM_LOG_DIR",
                Some(OsString::from_vec(vec![0x66, 0x6f, 0x80])),
            ),
        ]);

        let error = host_log_dir().expect_err("non-utf8 override should fail");
        assert!(matches!(
            error.code(),
            crate::error_codes::AtmErrorCode::ConfigHomeUnavailable
                | crate::error_codes::AtmErrorCode::ConfigParseFailed
                | crate::error_codes::AtmErrorCode::ConfigRetiredHookMembersKey
                | crate::error_codes::AtmErrorCode::ConfigRetiredLegacyHookKeys
                | crate::error_codes::AtmErrorCode::ConfigTeamParseFailed
                | crate::error_codes::AtmErrorCode::ConfigTeamMissing
        ));
        assert!(error.message().contains("UTF-8"));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn host_log_dir_rejects_overlong_override() {
        let home_dir = TempDir::new().expect("home");
        let too_long = format!("/{}", "a".repeat(MAX_HOST_LOG_DIR_UTF8_BYTES));
        let _env = LocalEnvGuard::set_many([
            ("ATM_LOG_DIR", Some(too_long.as_str())),
            ("HOME", Some(home_dir.path().to_str().expect("utf8 path"))),
        ]);

        let error = host_log_dir().expect_err("overlong ATM_LOG_DIR should fail");
        assert!(matches!(
            error.code(),
            crate::error_codes::AtmErrorCode::ConfigHomeUnavailable
                | crate::error_codes::AtmErrorCode::ConfigParseFailed
                | crate::error_codes::AtmErrorCode::ConfigRetiredHookMembersKey
                | crate::error_codes::AtmErrorCode::ConfigRetiredLegacyHookKeys
                | crate::error_codes::AtmErrorCode::ConfigTeamParseFailed
                | crate::error_codes::AtmErrorCode::ConfigTeamMissing
        ));
        assert!(error.message().contains("4096"));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn atm_home_rejects_non_absolute_override() {
        let home_dir = TempDir::new().expect("home");
        let _env = LocalEnvGuard::set_many([
            ("ATM_HOME", Some("relative/home")),
            ("HOME", Some(home_dir.path().to_str().expect("utf8 path"))),
        ]);

        let error = atm_home().expect_err("relative ATM_HOME should fail");
        assert_eq!(error.code(), crate::error::AtmErrorCode::AtmHomeUnresolved);
        assert!(error.message().contains("absolute path"));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn atm_home_rejects_non_utf8_override() {
        use std::os::unix::ffi::OsStringExt;

        let home_dir = TempDir::new().expect("home");
        let _env = LocalEnvGuard::set_many_os([
            (
                "HOME",
                Some(OsString::from(home_dir.path().to_str().expect("utf8 path"))),
            ),
            ("ATM_HOME", Some(OsString::from_vec(vec![0x66, 0x6f, 0x80]))),
        ]);

        let error = atm_home().expect_err("non-utf8 ATM_HOME should fail");
        assert_eq!(error.code(), crate::error::AtmErrorCode::AtmHomeUnresolved);
        assert!(error.message().contains("UTF-8"));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial(env)]
    fn atm_home_rejects_overlong_override() {
        let home_dir = TempDir::new().expect("home");
        let too_long = format!("/{}", "a".repeat(MAX_ATM_HOME_UTF8_BYTES));
        let _env = LocalEnvGuard::set_many([
            ("ATM_HOME", Some(too_long.as_str())),
            ("HOME", Some(home_dir.path().to_str().expect("utf8 path"))),
        ]);

        let error = atm_home().expect_err("overlong ATM_HOME should fail");
        assert_eq!(error.code(), crate::error::AtmErrorCode::AtmHomeUnresolved);
        assert!(error.message().contains("4096"));
    }
}
