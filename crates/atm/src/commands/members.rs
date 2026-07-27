#![allow(
    deprecated,
    reason = "the retained CLI members command still executes through the legacy atm-core roster boundary during the Phase AC transition"
)]

use anyhow::Result;
use atm_core::home;
use atm_core::team_admin::{self, MembersQuery};
use clap::Args;

use crate::commands::caller_context::{
    CallerContextOverrides, CallerTeamOverride, resolve_cli_caller_context,
};
use crate::commands::retained_roster::with_retained_roster_store;
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
        let json = self.json;
        let query = self.build_query()?;
        let outcome = with_retained_roster_store(|roster_store| {
            team_admin::list_members_with_roster_store(roster_store, query)
        })?;
        output::print_members_result(&outcome, json)
    }

    fn build_query(self) -> Result<MembersQuery> {
        let current_dir = home::command_invocation_dir()?;
        let caller_context = resolve_cli_caller_context(CallerContextOverrides {
            identity_override: None,
            chat_id_override: None,
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        Ok(MembersQuery {
            team: caller_context.caller_team,
            caller_identity: Some(caller_context.caller_identity),
            live_cwd: Some(current_dir),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use atm_core::boundary::{RosterEntry, RosterHarness, RosterMemberKind};
    use atm_core::home;
    use atm_core::schema::{AgentMember, TeamConfig};
    use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD, TEST_SENDER, TEST_TEAM};
    use atm_runtime_test_support::{
        SQLITE_RUNTIME_PATH_ENV, install_sqlite_retained_runtime_factory, open_sqlite_boundary,
    };
    use serial_test::serial;
    use tempfile::TempDir;

    use super::MembersCommand;
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
            let original = home::command_invocation_dir().expect("current dir");
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
            // This test module invokes the retained roster boundary directly.
            // Install its test-only factory here so correctness never depends on
            // another unit test having initialized global runtime state first.
            install_sqlite_retained_runtime_factory();
            let tempdir = TempDir::new().expect("tempdir");
            let home_dir = tempdir.path().to_path_buf();
            let sqlite_db_path = home_dir.join("runtime").join("mail.sqlite3");
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
            let assembly = open_sqlite_boundary(sqlite_db_path).expect("sqlite db");
            let team = TEST_TEAM
                .parse::<atm_core::types::TeamName>()
                .expect("team");
            let members = vec![
                RosterEntry {
                    team_name: team.clone(),
                    agent_name: ROLE_TEAM_LEAD.parse().expect("lead"),
                    member_kind: RosterMemberKind::Permanent,
                    harness: RosterHarness::ClaudeCode,
                    agent_type: atm_core::schema::AgentType::default(),
                    model: atm_core::types::ModelName::default(),
                    recipient_pane_id: None,
                    metadata_json: serde_json::Map::new(),
                },
                RosterEntry {
                    team_name: team.clone(),
                    agent_name: TEST_SENDER.parse().expect("sender"),
                    member_kind: RosterMemberKind::Permanent,
                    harness: RosterHarness::ClaudeCode,
                    agent_type: atm_core::schema::AgentType::default(),
                    model: atm_core::types::ModelName::default(),
                    recipient_pane_id: None,
                    metadata_json: serde_json::Map::new(),
                },
            ];
            assembly
                .roster_store_arc()
                .replace_roster(&team, &members)
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
                ("ATM_IDENTITY", Some(TEST_SENDER)),
                ("ATM_TEAM", None),
                ("HOME", Some(self.home_dir.to_str().expect("utf8"))),
                (
                    SQLITE_RUNTIME_PATH_ENV,
                    Some(
                        self.home_dir
                            .join("runtime")
                            .join("mail.sqlite3")
                            .to_str()
                            .expect("utf8"),
                    ),
                ),
            ]);
            let _cwd = CwdGuard::change_to(&self.current_dir);
            install_sqlite_retained_runtime_factory();
            f()
        }
    }

    #[test]
    #[serial(env)]
    fn build_query_preserves_team_override() {
        let command = MembersCommand {
            team: Some(TEST_TEAM.to_string()),
            json: true,
        };
        let _identity = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some("other-team")),
        ]);

        let query = command.build_query().expect("query");

        assert_eq!(query.team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial(env)]
    fn build_query_rejects_invalid_team_override() {
        let command = MembersCommand {
            team: Some("../evil".to_string()),
            json: false,
        };
        let _identity = EnvGuard::set_many([("ATM_IDENTITY", Some(TEST_SENDER))]);

        let error = command.build_query().expect_err("invalid team");

        assert!(error.to_string().contains("team name"));
    }

    #[test]
    #[serial(env)]
    fn build_query_uses_command_invocation_dir_for_live_cwd() {
        let fixture = Fixture::new();
        let command = MembersCommand {
            team: Some(TEST_TEAM.to_string()),
            json: false,
        };

        fixture.with_env_and_cwd(|| {
            let query = command.build_query().expect("query");
            let current_dir = home::command_invocation_dir().expect("current dir");
            assert_eq!(query.live_cwd.as_deref(), Some(current_dir.as_path()));
        });
    }

    #[test]
    #[serial(env)]
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
