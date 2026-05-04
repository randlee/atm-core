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
/// Tests that must exercise lead-role semantics should opt in explicitly by
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
