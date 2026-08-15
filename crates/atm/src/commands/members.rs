#![allow(
    deprecated,
    reason = "the retained CLI members command still executes through the legacy atm-core roster boundary during the Phase AC transition"
)]

use anyhow::Result;
use atm_core::doctor::DoctorQuery;
use atm_core::home;
use atm_core::protocol::{RuntimeMemberObservation, RuntimeMemberState, RuntimeStatusSnapshot};
use atm_core::team_admin::{self, MembersQuery};
use atm_core::types::TeamName;
use chrono::Utc;
use clap::Args;

use crate::commands::caller_context::{
    CallerContextOverrides, CallerTeamOverride, resolve_cli_caller_context,
};
use crate::commands::retained_roster::with_retained_roster_store;
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;

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
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let json = self.json;
        let query = self.build_query()?;
        let team = query.team.clone();
        let outcome = with_retained_roster_store(|roster_store| {
            team_admin::list_members_with_roster_store(roster_store, query)
        })?;
        let runtime = self.runtime_snapshot(&team, observability).await;
        print_members_result(&outcome, runtime.as_ref(), json)
    }

    fn build_query(&self) -> Result<MembersQuery> {
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

    async fn runtime_snapshot(
        &self,
        team: &TeamName,
        observability: &CliObservability,
    ) -> Option<RuntimeStatusSnapshot> {
        let (home_dir, current_dir) = resolve_command_runtime_context("members").ok()?;
        let caller_team =
            atm_core::caller_context::read_cli_team_from_env_or_warn("atm::members::runtime");
        let caller_identity =
            atm_core::caller_context::read_cli_identity_from_env_or_warn("atm::members::runtime");
        let query = DoctorQuery {
            home_dir,
            current_dir,
            team_override: Some(team.clone()),
            caller_team,
            caller_identity,
        };
        let composition = CliComposition::bootstrap(
            "members",
            observability,
            InvocationDir::new(&query.current_dir),
            AtmHomePath::new(&query.home_dir),
        )
        .ok()?;
        composition.doctor(query).await.ok()?.runtime_status
    }
}

/// Render a session id using the stable, bounded human-readable form required by AJ.6.
fn short_session_id_for_human(session_id: &atm_core::types::SessionId) -> String {
    let mut short = session_id.as_ref().chars().take(12).collect::<String>();
    if session_id.as_ref().chars().count() > 12 {
        short.push('…');
    }
    short
}

fn print_members_result(
    outcome: &atm_core::team_admin::MembersList,
    runtime: Option<&RuntimeStatusSnapshot>,
    json: bool,
) -> Result<()> {
    if json {
        let mut value = serde_json::to_value(outcome)?;
        if let Some(runtime) = runtime
            && let Some(members) = value
                .get_mut("members")
                .and_then(serde_json::Value::as_array_mut)
        {
            for member in members {
                let Some(name) = member.get("name").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let Some(observation) = runtime
                    .members
                    .iter()
                    .find(|observation| observation.member.as_str() == name)
                else {
                    continue;
                };
                let observation_value = serde_json::to_value(observation)?;
                if let Some(fields) = observation_value.as_object() {
                    for (key, field) in fields {
                        if key != "team" && key != "member" {
                            member[key] = field.clone();
                        }
                    }
                }
            }
        }
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    println!("Team: {}", outcome.team);
    if outcome.members.is_empty() {
        println!("  No members");
        return Ok(());
    }
    for member in &outcome.members {
        let home_dir = member.home_dir.as_path().display().to_string();
        let observation = runtime.and_then(|snapshot| {
            snapshot
                .members
                .iter()
                .find(|observation| observation.member == member.name)
        });
        let runtime_text = render_runtime_observation(observation);
        println!(
            "  {} | type={} harness={} model={} home_dir={} live_cwd={} pane={}{}",
            member.name,
            empty_dash(&member.agent_type),
            member.harness,
            empty_dash(&member.model),
            empty_dash(&home_dir),
            empty_dash_opt(member.live_cwd.as_deref()),
            empty_dash_opt(member.tmux_pane_id.as_deref()),
            runtime_text,
        );
    }
    Ok(())
}

fn render_runtime_observation(observation: Option<&RuntimeMemberObservation>) -> String {
    let Some(observation) = observation.filter(|value| {
        !(value.state == RuntimeMemberState::Unknown
            && value.session_id.is_none()
            && value.pid.is_none()
            && value.last_active_at.is_none()
            && value.state_changed_by.is_none()
            && value.state_changed_at.is_none()
            && value.session_changed_by.is_none()
            && value.session_changed_at.is_none())
    }) else {
        return String::new();
    };
    let state = serde_json::to_value(observation.state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned());
    let mut rendered = format!(" state={state}");
    if let Some(changed_at) = observation.state_changed_at {
        let age = (Utc::now() - changed_at.into_inner()).num_seconds().max(0);
        let age = if age < 60 {
            format!("{age}s")
        } else if age < 3_600 {
            format!("{}m", age / 60)
        } else {
            format!("{}h", age / 3_600)
        };
        rendered.push_str(&format!(" age={age}"));
    }
    if let Some(pid) = observation.pid {
        rendered.push_str(&format!(" pid={pid}"));
    }
    if let Some(session_id) = &observation.session_id {
        rendered.push_str(&format!(
            " session={}",
            short_session_id_for_human(session_id)
        ));
    }
    rendered
}

fn empty_dash(value: &impl std::fmt::Display) -> String {
    let rendered = value.to_string();
    if rendered.is_empty() {
        "-".to_owned()
    } else {
        rendered
    }
}

fn empty_dash_opt(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or("-")
        .to_owned()
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
        SQLITE_RUNTIME_PATH_ENV, install_sqlite_retained_runtime_factory,
        open_isolated_sqlite_boundary,
    };
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{MembersCommand, short_session_id_for_human};
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
            let assembly = open_isolated_sqlite_boundary(&home_dir).expect("sqlite db");
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
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime")
                .block_on(command.run(&CliObservability::fallback()))
                .expect("members run");
        });
    }

    #[test]
    fn short_session_id_uses_unicode_scalar_limit() {
        let session = atm_core::types::SessionId::new("ééééééééééééé").expect("session");
        assert_eq!(short_session_id_for_human(&session), "éééééééééééé…");
    }
}
