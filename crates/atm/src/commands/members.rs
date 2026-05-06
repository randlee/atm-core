use anyhow::Result;
use atm_core::home;
use atm_core::team_admin::{self, MembersQuery};
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// List the current member roster for one ATM team.
pub struct MembersCommand {
    #[arg(long)]
    team: Option<String>,

    #[arg(long)]
    json: bool,
}

impl MembersCommand {
    /// Execute the `atm members` command.
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        let home_dir = home::atm_home()?;
        let current_dir = std::env::current_dir()?;
        let json = self.json;
        let outcome = team_admin::list_members(self.build_query(home_dir, current_dir)?)?;
        output::print_members_result(&outcome, json)
    }

    fn build_query(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<MembersQuery> {
        Ok(MembersQuery {
            home_dir,
            current_dir,
            team_override: self.team.map(|value| value.parse()).transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use atm_core::schema::{AgentMember, TeamConfig};
    use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD, TEST_SENDER, TEST_TEAM};
    use serial_test::serial;
    use tempfile::TempDir;

    use super::MembersCommand;
    use crate::observability::CliObservability;

    struct Fixture {
        _tempdir: TempDir,
        home_dir: PathBuf,
        current_dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tempdir = TempDir::new().expect("tempdir");
            let home_dir = tempdir.path().to_path_buf();
            let current_dir = tempdir.path().join("workspace");
            fs::create_dir_all(&current_dir).expect("workspace");
            fs::write(
                current_dir.join(".atm.toml"),
                format!("[atm]\ndefault_team = \"{TEST_TEAM}\"\n"),
            )
            .expect("config");
            let team_dir = home_dir.join(".claude").join("teams").join(TEST_TEAM);
            fs::create_dir_all(&team_dir).expect("team dir");
            let config = TeamConfig {
                members: vec![
                    AgentMember::with_name(ROLE_TEAM_LEAD.parse().expect("lead")),
                    AgentMember::with_name(TEST_SENDER.parse().expect("sender")),
                ],
                ..Default::default()
            };
            fs::write(
                team_dir.join("config.json"),
                serde_json::to_vec(&config).expect("team config"),
            )
            .expect("write config");
            Self {
                _tempdir: tempdir,
                home_dir,
                current_dir,
            }
        }

        fn with_env_and_cwd<T>(&self, f: impl FnOnce() -> T) -> T {
            let _atm_home = EnvGuard::set_raw("ATM_HOME", self.home_dir.to_str().expect("utf8"));
            let original = std::env::current_dir().expect("current dir");
            std::env::set_current_dir(&self.current_dir).expect("set current dir");
            let result = f();
            std::env::set_current_dir(original).expect("restore current dir");
            result
        }
    }

    #[test]
    fn build_query_preserves_team_override() {
        let command = MembersCommand {
            team: Some("test-team".to_string()),
            json: true,
        };
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let current_dir = tempdir.path().join("cwd");

        let query = command
            .build_query(home_dir.clone(), current_dir.clone())
            .expect("query");

        assert_eq!(
            query.team_override.as_ref().map(|value| value.as_str()),
            Some("test-team")
        );
        assert_eq!(query.home_dir, home_dir);
        assert_eq!(query.current_dir, current_dir);
    }

    #[test]
    fn build_query_rejects_invalid_team_override() {
        let command = MembersCommand {
            team: Some("../evil".to_string()),
            json: false,
        };
        let tempdir = TempDir::new().expect("tempdir");

        let error = command
            .build_query(tempdir.path().join("home"), tempdir.path().join("cwd"))
            .expect_err("invalid team");

        assert!(error.to_string().contains("team name"));
    }

    #[test]
    #[serial]
    fn run_lists_member_roster_without_daemon() {
        let fixture = Fixture::new();
        let command = MembersCommand {
            team: Some(TEST_TEAM.to_string()),
            json: true,
        };

        fixture.with_env_and_cwd(|| {
            command
                .run(&CliObservability::fallback())
                .expect("members run");
        });
    }
}
