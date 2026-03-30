use std::env;
use std::path::PathBuf;

use crate::error::AtmError;

pub fn atm_home() -> Result<PathBuf, AtmError> {
    if let Some(home) = env::var_os("ATM_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }

    resolve_user_home().map(|home| home.join(".local").join("share").join("atm"))
}

pub fn team_dir(team: &str) -> Result<PathBuf, AtmError> {
    Ok(atm_home()?.join("teams").join(team))
}

pub fn inbox_path(team: &str, agent: &str) -> Result<PathBuf, AtmError> {
    Ok(team_dir(team)?.join("inbox").join(format!("{agent}.jsonl")))
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
    use std::env;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    use super::{atm_home, inbox_path, team_dir};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn atm_home_prefers_atm_home_env() {
        let _guard = env_lock().lock().expect("env lock");
        let original_atm_home = env::var_os("ATM_HOME");
        let original_home = env::var_os("HOME");

        env::set_var("ATM_HOME", "/tmp/atm-home");
        env::set_var("HOME", "/tmp/user-home");

        let resolved = atm_home().expect("atm home");
        assert_eq!(resolved, PathBuf::from("/tmp/atm-home"));

        restore("ATM_HOME", original_atm_home);
        restore("HOME", original_home);
    }

    #[test]
    fn atm_home_falls_back_to_local_share_atm() {
        let _guard = env_lock().lock().expect("env lock");
        let original_atm_home = env::var_os("ATM_HOME");
        let original_home = env::var_os("HOME");

        env::remove_var("ATM_HOME");
        env::set_var("HOME", "/tmp/fallback-home");

        let resolved = atm_home().expect("atm home");
        assert_eq!(
            resolved,
            PathBuf::from("/tmp/fallback-home/.local/share/atm")
        );

        restore("ATM_HOME", original_atm_home);
        restore("HOME", original_home);
    }

    #[test]
    fn team_and_inbox_paths_use_atm_home_layout() {
        let _guard = env_lock().lock().expect("env lock");
        let original_atm_home = env::var_os("ATM_HOME");

        env::set_var("ATM_HOME", "/tmp/atm-home");

        assert_eq!(
            team_dir("atm-dev").expect("team dir"),
            PathBuf::from("/tmp/atm-home/teams/atm-dev")
        );
        assert_eq!(
            inbox_path("atm-dev", "arch-ctm").expect("inbox path"),
            PathBuf::from("/tmp/atm-home/teams/atm-dev/inbox/arch-ctm.jsonl")
        );

        restore("ATM_HOME", original_atm_home);
    }

    fn restore(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
        }
    }
}
