#![allow(
    deprecated,
    reason = "the retained CLI teams command still executes through the legacy atm-core roster boundary during the Phase AC transition"
)]

use std::path::PathBuf;

use anyhow::Result;
use atm_core::home;
use atm_core::team_admin::{
    self, AddMemberRequest, BackupRequest, ClearNudgeTemplateOverrideRequest,
    DisableNudgeTemplateOverrideRequest, RemoveMemberRequest, RestoreRequest, RestoreResult,
    SetNudgeTemplateOverrideRequest, UpdateMemberRequest,
};
use atm_daemon_bootstrap::with_default_nudge_template_override_store;
use clap::{Args, Subcommand};

use crate::commands::caller_context::{
    CallerContext, CallerContextOverrides, resolve_cli_caller_context,
};
use crate::commands::retained_roster::with_retained_roster_store;
use crate::composition::reload_running_runtime_view;
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
    UpdateMember(UpdateMemberCommand),
    RemoveMember(RemoveMemberCommand),
    SetNudgeTemplate(SetNudgeTemplateCommand),
    DisableNudgeTemplate(DisableNudgeTemplateCommand),
    ClearNudgeTemplate(ClearNudgeTemplateCommand),
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

    #[arg(long = "home-dir")]
    home_dir: Option<PathBuf>,

    #[arg(
        long = "pane-id",
        help = "tmux pane id in '%<number>' form or a bare numeric pane id"
    )]
    pane_id: Option<String>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct UpdateMemberCommand {
    team: String,
    member: String,

    #[arg(long)]
    home_dir: Option<PathBuf>,

    #[arg(long = "workspace-root")]
    workspace_root: Option<PathBuf>,

    #[arg(long)]
    harness: Option<String>,

    #[arg(long)]
    agent_type: Option<String>,

    #[arg(long)]
    model: Option<String>,

    #[arg(
        long = "pane-id",
        help = "tmux pane id in '%<number>' form or a bare numeric pane id"
    )]
    pane_id: Option<String>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RemoveMemberCommand {
    team: String,
    member: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SetNudgeTemplateCommand {
    #[arg(long)]
    team: String,
    #[arg(long)]
    kind: String,

    #[arg(long = "template-body")]
    template_body: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct DisableNudgeTemplateCommand {
    #[arg(long)]
    team: String,
    #[arg(long)]
    kind: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ClearNudgeTemplateCommand {
    #[arg(long)]
    team: String,
    #[arg(long)]
    kind: String,

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
        let caller_context = resolve_cli_caller_context(CallerContextOverrides::default())?;
        let home_dir = home::atm_home()?;
        match self.command {
            None => {
                let outcome = with_retained_roster_store(|roster_store| {
                    team_admin::list_teams_with_roster_store(
                        roster_store,
                        caller_context.caller_team.clone(),
                    )
                })?;
                output::print_teams_result(&outcome, self.json)
            }
            Some(TeamsSubcommand::AddMember(command)) => command.run(home_dir),
            Some(TeamsSubcommand::UpdateMember(command)) => command.run(home_dir, caller_context),
            Some(TeamsSubcommand::RemoveMember(command)) => command.run(caller_context),
            Some(TeamsSubcommand::SetNudgeTemplate(command)) => command.run(caller_context),
            Some(TeamsSubcommand::DisableNudgeTemplate(command)) => command.run(caller_context),
            Some(TeamsSubcommand::ClearNudgeTemplate(command)) => command.run(caller_context),
            Some(TeamsSubcommand::Backup(command)) => command.run(home_dir),
            Some(TeamsSubcommand::Restore(command)) => command.run(home_dir),
        }
    }

    /// Publish a durable roster mutation into the daemon's immutable admission
    /// view before reporting the command complete. This uses the same
    /// authenticated control-plane reload as peer-trust mutations.
    fn reload_runtime_view() -> Result<()> {
        Ok(reload_running_runtime_view()?)
    }
}

impl AddMemberCommand {
    fn run(self, atm_home_dir: PathBuf) -> Result<()> {
        let json = self.json;
        let member_home_dir = self.resolve_member_home_dir()?;
        let request = self.build_request(atm_home_dir, member_home_dir)?;
        let outcome = with_retained_roster_store(|roster_store| {
            team_admin::add_member_with_roster_store(roster_store, request)
        })?;
        TeamsCommand::reload_runtime_view()?;
        output::print_add_member_result(&outcome, json)
    }

    fn resolve_member_home_dir(&self) -> Result<PathBuf> {
        match &self.home_dir {
            Some(path) => Ok(path.clone()),
            None => Ok(home::command_invocation_dir()?),
        }
    }

    fn build_request(
        self,
        atm_home_dir: PathBuf,
        member_home_dir: PathBuf,
    ) -> Result<AddMemberRequest> {
        AddMemberRequest::new(
            atm_home_dir,
            &self.team,
            &self.member,
            self.agent_type,
            self.model,
            member_home_dir,
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

impl SetNudgeTemplateCommand {
    fn run(self, caller_context: CallerContext) -> Result<()> {
        let json = self.json;
        let request = self.build_request(caller_context)?;
        let outcome = with_default_nudge_template_override_store(|override_store| {
            team_admin::set_nudge_template_override_with_store(override_store, request)
        })?;
        output::print_set_nudge_template_override_result(&outcome, json)
    }

    fn build_request(
        self,
        caller_context: CallerContext,
    ) -> Result<SetNudgeTemplateOverrideRequest> {
        SetNudgeTemplateOverrideRequest::new(
            caller_context.caller_team,
            &self.team,
            &self.kind,
            self.template_body,
        )
        .map_err(Into::into)
    }
}

impl DisableNudgeTemplateCommand {
    fn run(self, caller_context: CallerContext) -> Result<()> {
        let json = self.json;
        let request = self.build_request(caller_context)?;
        let outcome = with_default_nudge_template_override_store(|override_store| {
            team_admin::disable_nudge_template_override_with_store(override_store, request)
        })?;
        output::print_disable_nudge_template_override_result(&outcome, json)
    }

    fn build_request(
        self,
        caller_context: CallerContext,
    ) -> Result<DisableNudgeTemplateOverrideRequest> {
        DisableNudgeTemplateOverrideRequest::new(caller_context.caller_team, &self.team, &self.kind)
            .map_err(Into::into)
    }
}

impl ClearNudgeTemplateCommand {
    fn run(self, caller_context: CallerContext) -> Result<()> {
        let json = self.json;
        let request = self.build_request(caller_context)?;
        let outcome = with_default_nudge_template_override_store(|override_store| {
            team_admin::clear_nudge_template_override_with_store(override_store, request)
        })?;
        output::print_clear_nudge_template_override_result(&outcome, json)
    }

    fn build_request(
        self,
        caller_context: CallerContext,
    ) -> Result<ClearNudgeTemplateOverrideRequest> {
        ClearNudgeTemplateOverrideRequest::new(caller_context.caller_team, &self.team, &self.kind)
            .map_err(Into::into)
    }
}

impl UpdateMemberCommand {
    fn run(self, _atm_home_dir: PathBuf, caller_context: CallerContext) -> Result<()> {
        let json = self.json;
        let request = self.build_request(caller_context)?;
        let outcome = with_retained_roster_store(|roster_store| {
            team_admin::update_member_with_roster_store(roster_store, request)
        })?;
        TeamsCommand::reload_runtime_view()?;
        output::print_update_member_result(&outcome, json)
    }

    fn build_request(self, caller_context: CallerContext) -> Result<UpdateMemberRequest> {
        UpdateMemberRequest::new(
            caller_context.caller_identity,
            caller_context.caller_team,
            &self.team,
            &self.member,
            self.home_dir,
            self.workspace_root,
            self.harness,
            self.agent_type,
            self.model,
            self.pane_id,
        )
        .map_err(Into::into)
    }
}

impl RemoveMemberCommand {
    fn run(self, caller_context: CallerContext) -> Result<()> {
        let json = self.json;
        let request = self.build_request(caller_context)?;
        let outcome = with_retained_roster_store(|roster_store| {
            team_admin::remove_member_with_roster_store(roster_store, request)
        })?;
        TeamsCommand::reload_runtime_view()?;
        output::print_remove_member_result(&outcome, json)
    }

    fn build_request(self, caller_context: CallerContext) -> Result<RemoveMemberRequest> {
        RemoveMemberRequest::new(
            caller_context.caller_identity,
            caller_context.caller_team,
            &self.team,
            &self.member,
        )
        .map_err(Into::into)
    }
}

impl RestoreCommand {
    fn run(self, home_dir: PathBuf) -> Result<()> {
        let json = self.json;
        let request = self.build_request(home_dir)?;
        match with_retained_roster_store(|roster_store| {
            team_admin::restore_team_with_roster_store(roster_store, request)
        })? {
            RestoreResult::Applied(outcome) => {
                TeamsCommand::reload_runtime_view()?;
                output::print_restore_result(&outcome, json)
            }
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

    use atm_core::boundary::{RosterEntry, RosterHarness, RosterMemberKind};
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::home;
    use atm_core::schema::{AgentMember, TeamConfig};
    use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD, TEST_SENDER, TEST_TEAM};
    use atm_runtime_test_support::{
        SQLITE_RUNTIME_PATH_ENV, install_sqlite_retained_runtime_factory, open_sqlite_boundary,
    };
    use serial_test::serial;
    use tempfile::TempDir;

    use super::TeamsCommand;
    use super::{
        AddMemberCommand, BackupCommand, ClearNudgeTemplateCommand, DisableNudgeTemplateCommand,
        RemoveMemberCommand, RestoreCommand, SetNudgeTemplateCommand, TeamsSubcommand,
        UpdateMemberCommand,
    };
    use crate::commands::caller_context::CallerContext;
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

    fn temp_test_path(label: &str) -> (TempDir, PathBuf) {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join(label);
        (tempdir, path)
    }

    fn update_member_command(json: bool, home_dir: PathBuf) -> TeamsCommand {
        TeamsCommand {
            command: Some(TeamsSubcommand::UpdateMember(UpdateMemberCommand {
                team: TEST_TEAM.to_string(),
                member: TEST_SENDER.to_string(),
                home_dir: Some(home_dir),
                workspace_root: None,
                harness: Some("codex-cli".to_string()),
                agent_type: Some("worker".to_string()),
                model: Some("gpt-5".to_string()),
                pane_id: Some("%19".to_string()),
                json,
            })),
            json: false,
        }
    }

    fn set_nudge_template_command(json: bool, template_body: &str) -> TeamsCommand {
        TeamsCommand {
            command: Some(TeamsSubcommand::SetNudgeTemplate(SetNudgeTemplateCommand {
                team: TEST_TEAM.to_string(),
                kind: "delivery_ack".to_string(),
                template_body: template_body.to_string(),
                json,
            })),
            json: false,
        }
    }

    fn disable_nudge_template_command(json: bool) -> TeamsCommand {
        TeamsCommand {
            command: Some(TeamsSubcommand::DisableNudgeTemplate(
                DisableNudgeTemplateCommand {
                    team: TEST_TEAM.to_string(),
                    kind: "delivery_ack".to_string(),
                    json,
                },
            )),
            json: false,
        }
    }

    fn clear_nudge_template_command(json: bool) -> TeamsCommand {
        TeamsCommand {
            command: Some(TeamsSubcommand::ClearNudgeTemplate(
                ClearNudgeTemplateCommand {
                    team: TEST_TEAM.to_string(),
                    kind: "delivery_ack".to_string(),
                    json,
                },
            )),
            json: false,
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
                ("ATM_TEAM", Some(TEST_TEAM)),
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
    fn build_request_rejects_invalid_team_before_core() {
        let command = AddMemberCommand {
            team: "../evil".to_string(),
            member: TEST_SENDER.to_string(),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            home_dir: None,
            pane_id: None,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("invalid team");

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code(), AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn build_request_rejects_invalid_member_before_core() {
        let command = AddMemberCommand {
            team: TEST_TEAM.to_string(),
            member: "../evil".to_string(),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            home_dir: None,
            pane_id: None,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("invalid member");

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code(), AtmErrorCode::AddressParseFailed);
    }

    #[test]
    fn set_nudge_template_build_request_rejects_invalid_kind_before_core() {
        let command = SetNudgeTemplateCommand {
            team: TEST_TEAM.to_string(),
            kind: "not-a-kind".to_string(),
            template_body: "<atm/>".to_string(),
            json: false,
        };

        let error = command
            .build_request(CallerContext {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_chat_id: None,
                caller_team: TEST_TEAM.parse().expect("team"),
            })
            .expect_err("invalid kind");

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code(), AtmErrorCode::MessageValidationFailed);
    }

    #[test]
    fn set_nudge_template_build_request_rejects_empty_template_body_before_core() {
        let command = SetNudgeTemplateCommand {
            team: TEST_TEAM.to_string(),
            kind: "delivery_ack".to_string(),
            template_body: "   ".to_string(),
            json: false,
        };

        let error = command
            .build_request(CallerContext {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_chat_id: None,
                caller_team: TEST_TEAM.parse().expect("team"),
            })
            .expect_err("empty body");

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code(), AtmErrorCode::EmptyNudgeTemplateBody);
    }

    #[test]
    fn add_member_build_request_preserves_atm_and_member_home_dirs() {
        let command = AddMemberCommand {
            team: TEST_TEAM.to_string(),
            member: TEST_SENDER.to_string(),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            home_dir: None,
            pane_id: Some("17".to_string()),
            json: false,
        };

        let (_atm_home_guard, atm_home_dir) = temp_test_path("atm-home");
        let (_member_home_guard, member_home_dir) = temp_test_path("member-home");
        let request = command
            .build_request(atm_home_dir.clone(), member_home_dir.clone())
            .expect("request");

        assert_eq!(request.atm_home_dir.as_ref(), atm_home_dir.as_path());
        assert_eq!(request.member_home_dir.as_ref(), member_home_dir.as_path());
        assert_eq!(request.tmux_pane_id.as_deref(), Some("%17"));
    }

    #[test]
    #[serial(env)]
    fn add_member_defaults_member_home_dir_to_command_invocation_dir() {
        let fixture = Fixture::new();
        let command = AddMemberCommand {
            team: TEST_TEAM.to_string(),
            member: "new-member".to_string(),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            home_dir: None,
            pane_id: None,
            json: false,
        };

        fixture.with_env_and_cwd(|| {
            let member_home_dir = command.resolve_member_home_dir().expect("member home dir");
            let current_dir = home::command_invocation_dir().expect("current dir");
            assert_eq!(member_home_dir, current_dir);
        });
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
    fn update_member_build_request_preserves_target_and_caller_context() {
        let (_member_home_guard, member_home_dir) = temp_test_path("member-home");
        let command = UpdateMemberCommand {
            team: TEST_TEAM.to_string(),
            member: TEST_SENDER.to_string(),
            home_dir: Some(member_home_dir.clone()),
            workspace_root: None,
            harness: Some("codex-cli".to_string()),
            agent_type: Some("worker".to_string()),
            model: Some("gpt-5".to_string()),
            pane_id: Some("17".to_string()),
            json: true,
        };

        let request = command
            .build_request(CallerContext {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_chat_id: None,
                caller_team: TEST_TEAM.parse().expect("team"),
            })
            .expect("request");

        assert_eq!(request.team.as_str(), TEST_TEAM);
        assert_eq!(request.member.0.as_str(), TEST_SENDER);
        assert_eq!(request.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(request.caller_team.as_str(), TEST_TEAM);
        assert_eq!(
            request.home_dir.as_ref().map(AsRef::as_ref),
            Some(member_home_dir.as_path())
        );
        assert_eq!(request.tmux_pane_id.as_deref(), Some("%17"));
    }

    #[test]
    fn remove_member_build_request_preserves_target_and_caller_context() {
        let command = RemoveMemberCommand {
            team: TEST_TEAM.to_string(),
            member: TEST_SENDER.to_string(),
            json: true,
        };

        let request = command
            .build_request(CallerContext {
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_chat_id: None,
                caller_team: TEST_TEAM.parse().expect("team"),
            })
            .expect("request");

        assert_eq!(request.team.as_str(), TEST_TEAM);
        assert_eq!(request.member.as_str(), TEST_SENDER);
        assert_eq!(request.caller_identity.as_str(), TEST_SENDER);
        assert_eq!(request.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial_test::serial(env)]
    fn remove_member_rejects_cross_team_caller() {
        let fixture = Fixture::new();
        let command = RemoveMemberCommand {
            team: TEST_TEAM.to_string(),
            member: TEST_SENDER.to_string(),
            json: false,
        };

        fixture.with_env_and_cwd(|| {
            let error = command
                .run(CallerContext {
                    caller_identity: TEST_SENDER.parse().expect("caller"),
                    caller_chat_id: None,
                    caller_team: "other-team".parse().expect("team"),
                })
                .expect_err("cross-team caller");
            let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
            assert_eq!(atm_error.code(), AtmErrorCode::MessageValidationFailed);
            assert!(atm_error.message().contains("caller team"));
        });
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
    #[serial_test::serial(env)]
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
    #[serial_test::serial(env)]
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
    #[serial(env)]
    fn set_nudge_template_executes_through_shared_override_boundary() {
        let fixture = Fixture::new();

        fixture.with_env_and_cwd(|| {
            set_nudge_template_command(true, "<atm/>")
                .run(&CliObservability::fallback())
                .expect("set-nudge-template run");

            let saved = atm_daemon_bootstrap::with_default_nudge_template_override_store(
                |override_store| {
                    override_store.load_template_override(
                        &TEST_TEAM.parse().expect("team"),
                        "delivery_ack".parse().expect("kind"),
                    )
                },
            )
            .expect("load override")
            .expect("saved row");
            assert_eq!(saved.template_body(), Some("<atm/>"));
            assert!(!saved.is_disabled());
        });
    }

    #[test]
    #[serial(env)]
    fn disable_nudge_template_executes_through_shared_override_boundary() {
        let fixture = Fixture::new();

        fixture.with_env_and_cwd(|| {
            disable_nudge_template_command(true)
                .run(&CliObservability::fallback())
                .expect("disable-nudge-template run");

            let saved = atm_daemon_bootstrap::with_default_nudge_template_override_store(
                |override_store| {
                    override_store.load_template_override(
                        &TEST_TEAM.parse().expect("team"),
                        "delivery_ack".parse().expect("kind"),
                    )
                },
            )
            .expect("load override")
            .expect("saved row");
            assert!(saved.is_disabled());
        });
    }

    #[test]
    #[serial(env)]
    fn clear_nudge_template_executes_through_shared_override_boundary() {
        let fixture = Fixture::new();

        fixture.with_env_and_cwd(|| {
            set_nudge_template_command(true, "<atm/>")
                .run(&CliObservability::fallback())
                .expect("set-nudge-template run");
            clear_nudge_template_command(true)
                .run(&CliObservability::fallback())
                .expect("clear-nudge-template run");

            let saved = atm_daemon_bootstrap::with_default_nudge_template_override_store(
                |override_store| {
                    override_store.load_template_override(
                        &TEST_TEAM.parse().expect("team"),
                        "delivery_ack".parse().expect("kind"),
                    )
                },
            )
            .expect("load override");
            assert!(saved.is_none());
        });
    }

    #[test]
    #[serial(env)]
    fn update_member_requires_identity_from_environment() {
        let fixture = Fixture::new();
        let (_repair_guard, repaired_home_dir) = temp_test_path("repaired-home");
        let command = update_member_command(true, repaired_home_dir);
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(fixture.home_dir.to_str().expect("utf8"))),
            ("ATM_IDENTITY", None),
            ("ATM_TEAM", Some(TEST_TEAM)),
            ("HOME", Some(fixture.home_dir.to_str().expect("utf8"))),
        ]);
        let _cwd = CwdGuard::change_to(&fixture.current_dir);

        let error = command
            .run(&CliObservability::fallback())
            .expect_err("missing identity");

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code(), AtmErrorCode::IdentityUnavailable);
    }

    #[test]
    #[serial(env)]
    fn update_member_requires_team_from_environment_not_positional_target() {
        let fixture = Fixture::new();
        let (_repair_guard, repaired_home_dir) = temp_test_path("repaired-home");
        let command = update_member_command(true, repaired_home_dir);
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(fixture.home_dir.to_str().expect("utf8"))),
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", None),
            ("HOME", Some(fixture.home_dir.to_str().expect("utf8"))),
        ]);
        let _cwd = CwdGuard::change_to(&fixture.current_dir);

        let error = command
            .run(&CliObservability::fallback())
            .expect_err("missing team");

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code(), AtmErrorCode::TeamUnavailable);
    }

    #[test]
    #[serial(env)]
    fn update_member_rejects_invalid_identity_before_mutation() {
        let fixture = Fixture::new();
        let (_repair_guard, repaired_home_dir) = temp_test_path("repaired-home");
        let command = update_member_command(true, repaired_home_dir);
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(fixture.home_dir.to_str().expect("utf8"))),
            ("ATM_IDENTITY", Some("../bad")),
            ("ATM_TEAM", Some(TEST_TEAM)),
            ("HOME", Some(fixture.home_dir.to_str().expect("utf8"))),
        ]);
        let _cwd = CwdGuard::change_to(&fixture.current_dir);

        let error = command
            .run(&CliObservability::fallback())
            .expect_err("invalid identity");

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code(), AtmErrorCode::IdentityInvalid);
    }

    #[test]
    #[serial(env)]
    fn update_member_rejects_invalid_team_before_mutation() {
        let fixture = Fixture::new();
        let (_repair_guard, repaired_home_dir) = temp_test_path("repaired-home");
        let command = update_member_command(true, repaired_home_dir);
        let _env = EnvGuard::set_many([
            ("ATM_HOME", Some(fixture.home_dir.to_str().expect("utf8"))),
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some("../bad")),
            ("HOME", Some(fixture.home_dir.to_str().expect("utf8"))),
        ]);
        let _cwd = CwdGuard::change_to(&fixture.current_dir);

        let error = command
            .run(&CliObservability::fallback())
            .expect_err("invalid team");

        let atm_error = error.downcast_ref::<AtmError>().expect("AtmError");
        assert_eq!(atm_error.code(), AtmErrorCode::TeamInvalid);
    }
}
