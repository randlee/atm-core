use std::env;
use std::path::{Path, PathBuf};

use crate::error::AtmError;

pub fn atm_home() -> Result<PathBuf, AtmError> {
    if let Some(home) = env::var_os("ATM_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }

    resolve_user_home()
}

pub fn team_dir(team: &str) -> Result<PathBuf, AtmError> {
    team_dir_from_home(&atm_home()?, team)
}

pub fn inbox_path(team: &str, agent: &str) -> Result<PathBuf, AtmError> {
    inbox_path_from_home(&atm_home()?, team, agent)
}

pub fn team_dir_from_home(home_dir: &Path, team: &str) -> Result<PathBuf, AtmError> {
    Ok(home_dir.join(".claude").join("teams").join(team))
}

pub fn inbox_path_from_home(home_dir: &Path, team: &str, agent: &str) -> Result<PathBuf, AtmError> {
    Ok(team_dir_from_home(home_dir, team)?
        .join("inboxes")
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

    use super::{atm_home, inbox_path, team_dir};

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
            std::env::set_var(key, value);
            Self { key, original }
        }

        #[cfg(unix)]
        fn set_raw(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }

        #[cfg(unix)]
        fn remove(key: &'static str) -> Self {
            let original = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn atm_home_prefers_atm_home_env() {
        let _guard = env_lock().lock().expect("env lock");
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_home = EnvGuard::set("ATM_HOME", tempdir.path());

        let resolved = atm_home().expect("atm home");
        assert_eq!(resolved, tempdir.path());
    }

    #[cfg(unix)]
    #[test]
    fn atm_home_falls_back_to_home_dir() {
        let _guard = env_lock().lock().expect("env lock");
        let tempdir = TempDir::new().expect("tempdir");
        let _atm_home = EnvGuard::remove("ATM_HOME");
        let _home = EnvGuard::set_raw("HOME", tempdir.path().to_str().expect("utf8 path"));

        let resolved = atm_home().expect("atm home");
        assert_eq!(resolved, tempdir.path());
    }

    #[test]
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
}
