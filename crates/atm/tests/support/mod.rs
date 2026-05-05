#![allow(dead_code)]

use std::collections::BTreeMap;
#[cfg(test)]
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
#[cfg(test)]
use std::process::Output;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[allow(unused_imports)]
pub use atm_core::roles::ROLE_TEAM_LEAD;
use serde_json::json;
use tempfile::TempDir;

pub const TEST_TEAM: &str = "test-team";
pub const TEST_SENDER: &str = "sender-a";
pub const TEST_RECIPIENT: &str = "recipient";
pub const TEST_QA: &str = "qa-a";
pub const TEST_QA_AGENT: &str = TEST_QA;
pub const TEST_LEAD: &str = "test-lead";
pub const TEST_DAEMON: &str = "daemon";
pub const TEST_ORIGIN: &str = "host-a";
pub const TEST_SENDER_ADDRESS: &str = "sender-a@test-team";
pub const TEST_RECIPIENT_ADDRESS: &str = "recipient@test-team";
pub const TEST_LEAD_ADDRESS: &str = "test-lead@test-team";

#[cfg(test)]
pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub struct EnvGuard {
    key: String,
    original: Option<OsString>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl EnvGuard {
    pub fn set_raw(key: impl Into<String>, value: &str) -> Self {
        let key = key.into();
        let guard = env_lock().lock().expect("env lock");
        let original = std::env::var_os(&key);
        set_env_var(&key, value);
        Self {
            key,
            original,
            _guard: guard,
        }
    }
}

#[cfg(test)]
impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => set_env_var(&self.key, value),
            None => remove_env_var(&self.key),
        }
    }
}

#[cfg(test)]
pub fn set_env_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    // SAFETY: test callers acquire the shared test env lock before mutating
    // the process environment.
    unsafe { std::env::set_var(key, value) }
}

#[cfg(test)]
pub fn remove_env_var<K: AsRef<OsStr>>(key: K) {
    // SAFETY: test callers acquire the shared test env lock before mutating
    // the process environment.
    unsafe { std::env::remove_var(key) }
}

#[derive(Debug)]
pub struct TestEnv {
    pub tempdir: TempDir,
    pub env_map: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct TestEnvBuilder {
    team: String,
    members: Vec<String>,
    cwd_name: String,
}

impl TestEnvBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn team(mut self, team: impl Into<String>) -> Self {
        self.team = team.into();
        self
    }

    pub fn members<I, S>(mut self, members: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.members = members.into_iter().map(Into::into).collect();
        self
    }

    pub fn cwd_name(mut self, cwd_name: impl Into<String>) -> Self {
        self.cwd_name = cwd_name.into();
        self
    }

    pub fn build(self) -> io::Result<TestEnv> {
        let tempdir = tempfile::tempdir()?;
        let atm_home = tempdir.path().join("atm-home");
        let atm_config_home = tempdir.path().join("config-home");
        let atm_teams_dir = atm_config_home.join(".claude").join("teams");
        let team_dir = atm_teams_dir.join(&self.team);
        let inboxes_dir = team_dir.join("inboxes");
        let workflow_dir = team_dir.join(".atm-state").join("workflow");
        let db_dir = atm_home.join("db");
        let cwd = tempdir.path().join(&self.cwd_name);

        fs::create_dir_all(&atm_home)?;
        fs::create_dir_all(&inboxes_dir)?;
        fs::create_dir_all(&workflow_dir)?;
        fs::create_dir_all(&db_dir)?;
        fs::create_dir_all(&cwd)?;

        let config_path = team_dir.join("config.json");
        let members = self
            .members
            .into_iter()
            .map(|name| json!({ "name": name }))
            .collect::<Vec<_>>();
        fs::write(
            config_path,
            serde_json::to_vec_pretty(&json!({ "members": members }))?,
        )?;

        let env_map = BTreeMap::from([
            (
                "ATM_HOME".to_string(),
                atm_home.to_string_lossy().into_owned(),
            ),
            (
                "ATM_CONFIG_HOME".to_string(),
                atm_config_home.to_string_lossy().into_owned(),
            ),
            (
                "ATM_TEAMS_DIR".to_string(),
                atm_teams_dir.to_string_lossy().into_owned(),
            ),
        ]);

        Ok(TestEnv {
            tempdir,
            env_map,
            cwd,
        })
    }
}

/// Default fixtures use `TEST_LEAD` instead of the reserved `ROLE_TEAM_LEAD`
/// string so generic tests do not silently depend on production role naming.
/// Tests that must exercise `team-lead` semantics should opt in explicitly by
/// using `ROLE_TEAM_LEAD`.
impl Default for TestEnvBuilder {
    fn default() -> Self {
        Self {
            team: TEST_TEAM.to_string(),
            members: vec![
                TEST_SENDER.to_string(),
                TEST_RECIPIENT.to_string(),
                TEST_LEAD.to_string(),
            ],
            cwd_name: "cwd".to_string(),
        }
    }
}

pub fn qualified(agent: &str) -> String {
    format!("{agent}@{TEST_TEAM}")
}

pub fn configure_atm_command<'a>(
    command: &'a mut Command,
    home_dir: &std::path::Path,
    identity: Option<&str>,
) -> &'a mut Command {
    let daemon_bin = ensure_test_daemon_launcher(home_dir);
    command.env_clear();
    for key in [
        "PATH",
        "CARGO",
        "CARGO_HOME",
        "HOME",
        "RUSTUP_HOME",
        "USERPROFILE",
        "SYSTEMROOT",
        "TMPDIR",
        "TMP",
        "TEMP",
        "ComSpec",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("ATM_HOME", home_dir)
        .env("ATM_CONFIG_HOME", home_dir)
        .env("ATM_TEAM", TEST_TEAM)
        .env("ATM_DAEMON_BIN", &daemon_bin);
    if let Some(identity) = identity {
        command.env("ATM_IDENTITY", identity);
    }
    command
}

#[cfg(test)]
pub fn is_daemon_start_transient(output: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.contains("failed to read daemon request frame")
        || stderr.contains("daemon socket was not published")
        || stderr.contains("failed to connect to daemon socket")
}

fn ensure_test_daemon_launcher(home_dir: &std::path::Path) -> PathBuf {
    #[allow(unused_variables)]
    let hermetic_daemon = option_env!("CARGO_BIN_EXE_atm-daemon").map(PathBuf::from);
    if let Some(path) = hermetic_daemon.as_ref().filter(|path| path.exists()) {
        return path.clone();
    }

    let sibling = PathBuf::from(env!("CARGO_BIN_EXE_atm"))
        .with_file_name(format!("atm-daemon{}", std::env::consts::EXE_SUFFIX));
    if sibling.exists() {
        return sibling;
    }

    let _ = home_dir;
    panic!(
        "expected hermetic test daemon binary at one of: {:?}, {}",
        hermetic_daemon,
        sibling.display()
    );
}
