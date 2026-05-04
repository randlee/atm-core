#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde_json::json;
use tempfile::TempDir;

macro_rules! define_test_identities {
    ($team:literal, $sender:literal, $recipient:literal, $qa:literal, $lead:literal, $daemon:literal, $origin:literal) => {
        pub const TEST_TEAM: &str = $team;
        pub const TEST_SENDER: &str = $sender;
        pub const TEST_RECIPIENT: &str = $recipient;
        pub const TEST_QA: &str = $qa;
        pub const TEST_QA_AGENT: &str = TEST_QA;
        #[allow(unused_imports)]
        pub use atm_core::roles::ROLE_TEAM_LEAD;
        pub const TEST_LEAD: &str = $lead;
        pub const TEST_DAEMON: &str = $daemon;
        pub const TEST_ORIGIN: &str = $origin;
        pub const TEST_SENDER_ADDRESS: &str = concat!($sender, "@", $team);
        pub const TEST_RECIPIENT_ADDRESS: &str = concat!($recipient, "@", $team);
        pub const TEST_LEAD_ADDRESS: &str = concat!($lead, "@", $team);
    };
}

define_test_identities!(
    "test-team",
    "sender-a",
    "recipient",
    "qa-a",
    "test-lead",
    "daemon",
    "host-a"
);

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

    pub fn members(mut self, members: &[&str]) -> Self {
        self.members = members.iter().map(|member| (*member).to_string()).collect();
        self
    }

    pub fn cwd_name(mut self, cwd_name: impl Into<String>) -> Self {
        self.cwd_name = cwd_name.into();
        self
    }

    pub fn build(self) -> io::Result<TestEnv> {
        let tempdir = tempfile::tempdir()?;
        let home_dir = tempdir.path().join("home");
        let config_home = tempdir.path().join("config");
        let teams_dir = config_home.join("teams");
        let team_dir = teams_dir.join(&self.team);
        let inbox_dir = team_dir.join("inbox");
        let cwd = tempdir.path().join(self.cwd_name);

        fs::create_dir_all(&home_dir)?;
        fs::create_dir_all(&inbox_dir)?;
        fs::create_dir_all(&cwd)?;

        let mut members = self.members;
        if members.is_empty() {
            members.push(TEST_SENDER.to_string());
        }

        let config = json!({
            "teamName": self.team,
            "members": members.into_iter().map(|name| json!({"name": name})).collect::<Vec<_>>(),
        });
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec_pretty(&config).expect("serialize config"),
        )?;

        let mut env_map = BTreeMap::new();
        env_map.insert("ATM_HOME".to_string(), home_dir.display().to_string());
        env_map.insert(
            "ATM_CONFIG_HOME".to_string(),
            config_home.display().to_string(),
        );
        env_map.insert("ATM_TEAMS_DIR".to_string(), teams_dir.display().to_string());
        env_map.insert("ATM_TEAM".to_string(), self.team);

        Ok(TestEnv {
            tempdir,
            env_map,
            cwd,
        })
    }
}

impl Default for TestEnvBuilder {
    fn default() -> Self {
        Self {
            team: TEST_TEAM.to_string(),
            members: vec![TEST_SENDER.to_string()],
            cwd_name: "repo".to_string(),
        }
    }
}
