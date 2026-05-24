use std::path::PathBuf;

use anyhow::Result;
use atm_core::home;
use atm_core::team_admin::{self, AddMemberRequest, BackupRequest, RestoreRequest, RestoreResult};
use clap::{Args, Subcommand};

use crate::commands::retained_roster::with_retained_roster_store;
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
                let current_dir = std::env::current_dir()?;
                let outcome = with_retained_roster_store(|roster_store| {
                    team_admin::list_teams_with_roster_store(roster_store, current_dir)
                })?;
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
        let outcome = with_retained_roster_store(|roster_store| {
            team_admin::add_member_with_roster_store(roster_store, request)
        })?;
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
        let request = self.build_request(home_dir)?;
        let outcome = with_retained_roster_store(|roster_store| {
            team_admin::backup_team_with_roster_store(roster_store, request)
        })?;
        output::print_backup_result(&outcome, json)
    }

    fn build_request(self, home_dir: PathBuf) -> Result<BackupRequest> {
        BackupRequest::new(home_dir, &self.team).map_err(Into::into)
    }
}

impl RestoreCommand {
    fn run(self, home_dir: PathBuf) -> Result<()> {
        let json = self.json;
        let request = self.build_request(home_dir)?;
        match with_retained_roster_store(|roster_store| {
            team_admin::restore_team_with_roster_store(roster_store, request)
        })? {
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

    use atm_core::boundary::{
        ReplaySource, RosterHarness, RosterMemberKind, RosterMemberRecord,
        RosterStoreReplaceRosterRequest,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::schema::{AgentMember, TeamConfig};
    use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD, TEST_SENDER, TEST_TEAM};
    use atm_runtime_test_support::open_sqlite_boundary;
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

    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn change_to(path: &std::path::Path) -> Self {
            let original = std::env::current_dir().expect("current dir");
            std::env::set_current_dir(path).expect("set current dir");
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.original).expect("restore current dir");
        }
    }

    impl Fixture {
        fn new() -> Self {
            let tempdir = TempDir::new().expect("tempdir");
            let home_dir = tempdir.path().to_path_buf();
            let sqlite_db_path = atm_core::home::host_mail_db_path_from_home(&home_dir);
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
            let assembly = open_sqlite_boundary(sqlite_db_path).expect("sqlite db");
            assembly
                .roster_store()
                .replace_roster(RosterStoreReplaceRosterRequest {
                    team: TEST_TEAM.parse().expect("team"),
                    members: vec![
                        RosterMemberRecord {
                            team_name: TEST_TEAM.parse().expect("team"),
                            agent_name: ROLE_TEAM_LEAD.parse().expect("lead"),
                            member_kind: RosterMemberKind::Permanent,
                            harness: RosterHarness::ClaudeCode,
                            agent_type: String::new(),
                            model: String::new(),
                            recipient_pane_id: None,
                            metadata_json: serde_json::Map::new(),
                        },
                        RosterMemberRecord {
                            team_name: TEST_TEAM.parse().expect("team"),
                            agent_name: TEST_SENDER.parse().expect("sender"),
                            member_kind: RosterMemberKind::Permanent,
                            harness: RosterHarness::ClaudeCode,
                            agent_type: String::new(),
                            model: String::new(),
                            recipient_pane_id: None,
                            metadata_json: serde_json::Map::new(),
                        },
                    ],
                    source: Some(ReplaySource::new("teams-test").expect("source")),
                })
                .expect("seed roster");

            Self {
                _tempdir: tempdir,
                home_dir,
                current_dir,
            }
        }

        fn with_env_and_cwd<T>(&self, f: impl FnOnce() -> T) -> T {
            let _env = EnvGuard::set_many([
                ("ATM_HOME", Some(self.home_dir.to_str().expect("utf8"))),
                ("ATM_TEAM", None),
                ("HOME", Some(self.home_dir.to_str().expect("utf8"))),
            ]);
            let _cwd = CwdGuard::change_to(&self.current_dir);
            f()
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

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code, AtmErrorCode::AddressParseFailed);
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

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code, AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn backup_build_request_preserves_team() {
        let command = BackupCommand {
            team: TEST_TEAM.to_string(),
            json: true,
        };

        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let request = command.build_request(home_dir.clone()).expect("request");

        assert_eq!(request.team.as_str(), TEST_TEAM);
        assert_eq!(request.home_dir, home_dir);
    }

    #[test]
    fn restore_build_request_preserves_from_path_and_dry_run() {
        let tempdir = TempDir::new().expect("tempdir");
        let backup_path = tempdir.path().join("backup");
        let command = RestoreCommand {
            team: TEST_TEAM.to_string(),
            from: Some(backup_path.clone()),
            dry_run: true,
            json: false,
        };

        let home_dir = tempdir.path().join("home");
        let request = command.build_request(home_dir.clone()).expect("request");

        assert_eq!(request.team.as_str(), TEST_TEAM);
        assert_eq!(request.home_dir, home_dir);
        assert_eq!(request.from, Some(backup_path));
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

    #[test]
    #[serial]
    fn add_member_executes_without_default_runtime_factory() {
        let fixture = Fixture::new();

        fixture.with_env_and_cwd(|| {
            AddMemberCommand {
                team: TEST_TEAM.to_string(),
                member: "new-member".to_string(),
                agent_type: "general-purpose".to_string(),
                model: "unknown".to_string(),
                cwd: Some(fixture.current_dir.clone()),
                pane_id: Some("%17".to_string()),
                json: true,
            }
            .run(fixture.home_dir.clone())
            .expect("add-member run");
        });
    }
}
