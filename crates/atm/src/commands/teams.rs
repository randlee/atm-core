use std::path::PathBuf;

use anyhow::Result;
use atm_core::home;
use atm_core::team_admin::{self, AddMemberRequest, BackupRequest, RestoreRequest, RestoreResult};
use clap::{Args, Subcommand};

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// List teams or run one team-administration subcommand.
pub struct TeamsCommand {
    #[command(subcommand)]
    command: Option<TeamsSubcommand>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum TeamsSubcommand {
    AddMember(AddMemberCommand),
    Backup(BackupCommand),
    Restore(RestoreCommand),
}

#[derive(Debug, Args)]
struct AddMemberCommand {
    team: String,
    member: String,

    #[arg(long, default_value = "general-purpose")]
    agent_type: String,

    #[arg(long, default_value = "unknown")]
    model: String,

    #[arg(long)]
    cwd: Option<PathBuf>,

    #[arg(
        long = "pane-id",
        help = "tmux pane id in '%<number>' form or a bare numeric pane id"
    )]
    pane_id: Option<String>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct BackupCommand {
    team: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RestoreCommand {
    team: String,

    #[arg(long)]
    from: Option<PathBuf>,

    #[arg(long = "dry-run")]
    dry_run: bool,

    #[arg(long)]
    json: bool,
}

impl TeamsCommand {
    /// Execute the `atm teams` command.
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        let home_dir = home::atm_home()?;
        match self.command {
            None => {
                let outcome = team_admin::list_teams(home_dir, std::env::current_dir()?)?;
                output::print_teams_result(&outcome, self.json)
            }
            Some(TeamsSubcommand::AddMember(command)) => command.run(home_dir),
            Some(TeamsSubcommand::Backup(command)) => command.run(home_dir),
            Some(TeamsSubcommand::Restore(command)) => command.run(home_dir),
        }
    }
}

impl AddMemberCommand {
    fn run(self, home_dir: PathBuf) -> Result<()> {
        let json = self.json;
        let cwd = match self.cwd.clone() {
            Some(path) => path,
            None => std::env::current_dir()?,
        };
        let request = self.build_request(home_dir, cwd)?;
        let outcome = team_admin::add_member(request)?;
        output::print_add_member_result(&outcome, json)
    }

    fn build_request(self, home_dir: PathBuf, cwd: PathBuf) -> Result<AddMemberRequest> {
        AddMemberRequest::new(
            home_dir,
            &self.team,
            &self.member,
            self.agent_type,
            self.model,
            cwd,
            self.pane_id,
        )
        .map_err(Into::into)
    }
}

impl BackupCommand {
    fn run(self, home_dir: PathBuf) -> Result<()> {
        let json = self.json;
        let outcome = team_admin::backup_team(self.build_request(home_dir)?)?;
        output::print_backup_result(&outcome, json)
    }

    fn build_request(self, home_dir: PathBuf) -> Result<BackupRequest> {
        BackupRequest::new(home_dir, &self.team).map_err(Into::into)
    }
}

impl RestoreCommand {
    fn run(self, home_dir: PathBuf) -> Result<()> {
        let json = self.json;
        match team_admin::restore_team(self.build_request(home_dir)?)? {
            RestoreResult::Applied(outcome) => output::print_restore_result(&outcome, json),
            RestoreResult::DryRun(plan) => output::print_restore_plan(&plan, json),
        }
    }

    fn build_request(self, home_dir: PathBuf) -> Result<RestoreRequest> {
        RestoreRequest::new(home_dir, &self.team, self.from, self.dry_run).map_err(Into::into)
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

    use super::TeamsCommand;
    use super::{AddMemberCommand, BackupCommand, RestoreCommand};
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
            fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes");
            fs::create_dir_all(home_dir.join(".claude").join("tasks").join(TEST_TEAM))
                .expect("tasks");
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
            fs::write(
                team_dir.join("inboxes").join(format!("{TEST_SENDER}.json")),
                "[]",
            )
            .expect("write inbox");

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
    fn build_request_rejects_invalid_team_before_core() {
        let command = AddMemberCommand {
            team: "../evil".to_string(),
            member: TEST_SENDER.to_string(),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            cwd: None,
            pane_id: None,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("invalid team");

        assert!(error.to_string().contains("team name"));
    }

    #[test]
    fn build_request_rejects_invalid_member_before_core() {
        let command = AddMemberCommand {
            team: TEST_TEAM.to_string(),
            member: "../evil".to_string(),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            cwd: None,
            pane_id: None,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("invalid member");

        assert!(error.to_string().contains("agent name"));
    }

    #[test]
    fn backup_build_request_preserves_team() {
        let command = BackupCommand {
            team: TEST_TEAM.to_string(),
            json: true,
        };

        let request = command
            .build_request(PathBuf::from("/tmp/home"))
            .expect("request");

        assert_eq!(request.team.as_str(), TEST_TEAM);
        assert_eq!(request.home_dir, PathBuf::from("/tmp/home"));
    }

    #[test]
    fn restore_build_request_preserves_from_path_and_dry_run() {
        let command = RestoreCommand {
            team: TEST_TEAM.to_string(),
            from: Some(PathBuf::from("/tmp/backup")),
            dry_run: true,
            json: false,
        };

        let request = command
            .build_request(PathBuf::from("/tmp/home"))
            .expect("request");

        assert_eq!(request.team.as_str(), TEST_TEAM);
        assert_eq!(request.home_dir, PathBuf::from("/tmp/home"));
        assert_eq!(request.from, Some(PathBuf::from("/tmp/backup")));
        assert!(request.dry_run);
    }

    #[test]
    #[serial]
    fn teams_run_lists_discovered_teams_without_daemon() {
        let fixture = Fixture::new();
        let command = TeamsCommand {
            command: None,
            json: true,
        };

        fixture.with_env_and_cwd(|| {
            command
                .run(&CliObservability::fallback())
                .expect("teams run");
        });
    }

    #[test]
    #[serial]
    fn backup_and_restore_dry_run_execute_without_daemon() {
        let fixture = Fixture::new();

        fixture.with_env_and_cwd(|| {
            BackupCommand {
                team: TEST_TEAM.to_string(),
                json: true,
            }
            .run(fixture.home_dir.clone())
            .expect("backup run");
        });

        let backup_root = fixture
            .home_dir
            .join(".claude")
            .join("teams")
            .join(".backups")
            .join(TEST_TEAM);
        let backup_dir = fs::read_dir(&backup_root)
            .expect("backup root")
            .next()
            .expect("backup entry")
            .expect("backup dir")
            .path();

        fixture.with_env_and_cwd(|| {
            RestoreCommand {
                team: TEST_TEAM.to_string(),
                from: Some(backup_dir),
                dry_run: true,
                json: true,
            }
            .run(fixture.home_dir.clone())
            .expect("restore run");
        });
    }
}
