use crate::observability::CliObservability;
use crate::output;
use anyhow::Result;
use atm_core::doctor::{self, DaemonRuntimeDoctorReport, DoctorAliasMismatch, DoctorQuery};
use atm_core::team_admin::MembersList;
use atm_core::types::TeamName;
#[cfg(not(test))]
use atm_daemon_bootstrap::assemble_default_runtime;
#[cfg(test)]
use atm_runtime_test_support::open_sqlite_boundary;
use clap::Args;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};

#[derive(Debug, Args)]
/// Run ATM health and configuration diagnostics.
pub struct DoctorCommand {
    #[arg(long, help = "Override the resolved team for the doctor check.")]
    team: Option<String>,

    #[arg(
        long,
        conflicts_with = "team",
        help = "Inspect every team in the canonical roster."
    )]
    all_teams: bool,

    #[arg(long, help = "Emit the doctor report as JSON.")]
    json: bool,
}

impl DoctorCommand {
    // L.5 disposition (UNI-003): keep DoctorCommand injectability deferred for
    // initial release. Current service-level coverage exercises doctor behavior
    // without introducing a wider command abstraction before a concrete need
    // appears.
    /// Execute the `atm doctor` command.
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("doctor")?;
        let json = self.json;
        let report = self.execute(observability, home_dir, current_dir).await?;

        let has_errors = report.has_errors();
        output::print_doctor_result(&report, json)?;
        if has_errors {
            std::process::exit(1);
        }
        Ok(())
    }

    fn build_query(
        &self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<DoctorQuery> {
        let team_override = self
            .team
            .as_ref()
            .map(|value| value.parse::<atm_core::types::TeamName>())
            .transpose()?;
        // Capture the invoking CLI process's identity here, where the process
        // environment is genuinely the caller's. When the doctor request is
        // serviced over IPC by the long-lived daemon, the daemon cannot read
        // these values from its own frozen launch-time environment, so they
        // must ride along in the request payload.
        let caller_team =
            atm_core::caller_context::read_cli_team_from_env_or_warn("atm::doctor::build_query");
        let caller_identity = atm_core::caller_context::read_cli_identity_from_env_or_warn(
            "atm::doctor::build_query",
        );
        Ok(DoctorQuery {
            home_dir,
            current_dir,
            team_override,
            all_teams: self.all_teams,
            caller_team,
            caller_identity,
        })
    }

    async fn execute(
        self,
        observability: &CliObservability,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<atm_core::doctor::DoctorReport> {
        let local_report =
            self.execute_direct_local(observability, home_dir.clone(), current_dir.clone())?;
        let query = self.build_query(home_dir.clone(), current_dir)?;

        let mut report = match CliComposition::bootstrap(
            "doctor",
            observability,
            InvocationDir::new(&query.current_dir),
            AtmHomePath::new(&query.home_dir),
        ) {
            Ok(composition) => composition
                .doctor(query.clone())
                .await
                .map_err(anyhow::Error::from),
            Err(_) => Ok(local_report),
        }?;
        append_pane_alias_mismatches(&mut report, &query);
        Ok(report)
    }

    fn execute_direct_local(
        &self,
        observability: &CliObservability,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<atm_core::doctor::DoctorReport> {
        let query = self.build_query(home_dir.clone(), current_dir)?;
        #[cfg(not(test))]
        let runtime = assemble_default_runtime()?;
        #[cfg(test)]
        let runtime = open_sqlite_boundary(home_dir.join(".atm").join("db").join("mail.db"))?;
        let (peer_config, peer_findings) =
            doctor::peer_config_doctor_report(runtime.peer_config_store().as_ref());
        doctor::run_doctor_with_runtime_ports(
            query,
            observability,
            &runtime.service_runtime,
            &runtime.doctor_ports,
            Some(DaemonRuntimeDoctorReport {
                findings: peer_findings,
                peer_config: Some(peer_config),
            }),
        )
        .map_err(anyhow::Error::from)
    }
}

/// The only Rust reader of rmux pane aliases. The values remain rmux/spawner
/// data; doctor compares them with durable roster aliases but never uses them
/// for ATM ingress, resolution, sends, or writes.
#[derive(Debug, Deserialize)]
struct PaneAliasConfig {
    #[serde(default)]
    rmux: RmuxConfig,
}

#[derive(Debug, Default, Deserialize)]
struct RmuxConfig {
    #[serde(default)]
    windows: Vec<RmuxWindow>,
}

#[derive(Debug, Default, Deserialize)]
struct RmuxWindow {
    #[serde(default)]
    panes: Vec<RmuxPane>,
}

#[derive(Debug, Deserialize)]
struct RmuxPane {
    name: String,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn append_pane_alias_mismatches(report: &mut atm_core::doctor::DoctorReport, query: &DoctorQuery) {
    let Some(workspace_team) = caller_workspace_team(query) else {
        return;
    };
    let Some(path) = discover_atm_toml(&query.current_dir) else {
        return;
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(config) = toml::from_str::<PaneAliasConfig>(&contents) else {
        return;
    };
    let Some(roster) = roster_for_workspace_team(report, &workspace_team) else {
        return;
    };

    let roster_aliases = roster
        .members
        .iter()
        .map(|member| (member.name.as_str().to_owned(), member.alias.clone()))
        .collect::<BTreeMap<_, _>>();
    report.alias_mismatches = pane_alias_mismatches(config, &workspace_team, &roster_aliases);
}

fn caller_workspace_team(query: &DoctorQuery) -> Option<TeamName> {
    query.caller_team.clone()
}

fn discover_atm_toml(current_dir: &Path) -> Option<PathBuf> {
    current_dir
        .ancestors()
        .map(|directory| directory.join(".atm.toml"))
        .find(|path| path.is_file())
}

fn roster_for_workspace_team<'a>(
    report: &'a atm_core::doctor::DoctorReport,
    workspace_team: &TeamName,
) -> Option<&'a MembersList> {
    report
        .member_roster
        .as_ref()
        .filter(|roster| roster.team == *workspace_team)
        .or_else(|| {
            report
                .team_rosters
                .iter()
                .find(|roster| roster.team == *workspace_team)
        })
}

fn pane_alias_mismatches(
    config: PaneAliasConfig,
    workspace_team: &TeamName,
    roster_aliases: &BTreeMap<String, Option<String>>,
) -> Vec<DoctorAliasMismatch> {
    config
        .rmux
        .windows
        .into_iter()
        .flat_map(|window| window.panes)
        .filter_map(|pane| {
            let config_alias = pane.alias?;
            if pane.env.get("ATM_TEAM").map(String::as_str) != Some(workspace_team.as_str()) {
                return None;
            }
            let roster_alias = roster_aliases.get(&pane.name).cloned().flatten();
            let member = pane.name.parse().ok()?;
            (roster_alias.as_deref() != Some(config_alias.as_str())).then(|| DoctorAliasMismatch {
                team: workspace_team.clone(),
                member,
                config_alias,
                roster_alias,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use atm_core::error::AtmError;
    use atm_core::test_support::EnvGuard;
    use clap::{Parser, error::ErrorKind};
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{DoctorCommand, PaneAliasConfig, discover_atm_toml, pane_alias_mismatches};
    use crate::observability::CliObservability;

    #[derive(Debug, Parser)]
    struct DoctorCli {
        #[command(flatten)]
        doctor: DoctorCommand,
    }

    #[test]
    fn all_teams_conflicts_with_team_override() {
        let error = DoctorCli::try_parse_from(["doctor", "--team", "team-a", "--all-teams"])
            .expect_err("team and all-teams must conflict");

        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    fn test_paths() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let current_dir = tempdir.path().join("cwd");
        (tempdir, home_dir, current_dir)
    }

    fn pane_config(panes: &str) -> PaneAliasConfig {
        toml::from_str(&format!("[rmux]\n[[rmux.windows]]\n{panes}",)).expect("pane config")
    }

    fn workspace_team() -> atm_core::types::TeamName {
        "workspace".parse().expect("team")
    }

    fn alias_map(
        entries: &[(&str, Option<&str>)],
    ) -> std::collections::BTreeMap<String, Option<String>> {
        entries
            .iter()
            .map(|(name, alias)| ((*name).to_owned(), alias.map(str::to_owned)))
            .collect()
    }

    #[test]
    fn pane_alias_mismatch_omits_matching_alias() {
        let config = pane_config(
            r#"[[rmux.windows.panes]]
name = "member-a"
alias = "alias-a"
env = { ATM_TEAM = "workspace" }
"#,
        );

        let mismatches = pane_alias_mismatches(
            config,
            &workspace_team(),
            &alias_map(&[("member-a", Some("alias-a"))]),
        );

        assert!(mismatches.is_empty());
    }

    #[test]
    fn pane_alias_mismatch_reports_differing_and_absent_roster_aliases() {
        let config = pane_config(
            r#"[[rmux.windows.panes]]
name = "member-a"
alias = "alias-a"
env = { ATM_TEAM = "workspace" }

[[rmux.windows.panes]]
name = "member-b"
alias = "alias-b"
env = { ATM_TEAM = "workspace" }
"#,
        );

        let mismatches = pane_alias_mismatches(
            config,
            &workspace_team(),
            &alias_map(&[("member-a", Some("wrong")), ("member-b", None)]),
        );

        assert_eq!(mismatches.len(), 2);
        assert_eq!(mismatches[0].member.as_str(), "member-a");
        assert_eq!(mismatches[0].config_alias, "alias-a");
        assert_eq!(mismatches[0].roster_alias.as_deref(), Some("wrong"));
        assert_eq!(mismatches[1].member.as_str(), "member-b");
        assert_eq!(mismatches[1].roster_alias, None);
    }

    #[test]
    fn pane_alias_mismatch_ignores_panes_without_alias_or_from_other_teams() {
        let config = pane_config(
            r#"[[rmux.windows.panes]]
name = "member-a"
env = { ATM_TEAM = "workspace" }

[[rmux.windows.panes]]
name = "member-b"
alias = "other-alias"
env = { ATM_TEAM = "other" }
"#,
        );

        let mismatches = pane_alias_mismatches(
            config,
            &workspace_team(),
            &alias_map(&[("member-a", None), ("member-b", None)]),
        );

        assert!(mismatches.is_empty());
    }

    #[test]
    fn pane_alias_check_skips_when_no_atm_toml_is_discovered() {
        let temporary_directory = TempDir::new().expect("temporary directory");

        assert_eq!(discover_atm_toml(temporary_directory.path()), None);
    }

    #[test]
    fn build_query_preserves_team_override() {
        let command = DoctorCommand {
            team: Some("test-team".to_string()),
            all_teams: false,
            json: true,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let query = command.build_query(home_dir, current_dir).expect("query");

        assert_eq!(
            query.team_override.as_ref().map(|value| value.as_str()),
            Some("test-team")
        );
    }

    #[test]
    fn build_query_adds_recovery_for_invalid_team_override() {
        let command = DoctorCommand {
            team: Some("bad team".to_string()),
            all_teams: false,
            json: false,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let error = command
            .build_query(home_dir, current_dir)
            .expect_err("invalid team override should fail");
        let atm_error = error.downcast_ref::<AtmError>().expect("atm error");

        assert!(atm_error.message().contains("Recovery:"));
    }

    #[test]
    #[serial(env)]
    fn execute_runs_direct_local_doctor_path_without_inherited_bootstrap_configuration() {
        let observability = CliObservability::fallback();
        let command = DoctorCommand {
            team: None,
            all_teams: false,
            json: false,
        };
        let (_tempdir, home_dir, current_dir) = test_paths();
        std::fs::create_dir_all(&home_dir).expect("home dir");
        std::fs::create_dir_all(&current_dir).expect("current dir");
        std::fs::create_dir_all(home_dir.join(".atm").join("db")).expect("host db dir");
        // Clear every
        // bootstrap input that could otherwise make this unit test connect to
        // or launch a caller-selected daemon.
        let _env = EnvGuard::set_many([
            ("ATM_DAEMON_BIN", None),
            ("ATM_DAEMON_SOCKET", None),
            ("ATM_HOME", None),
            ("ATM_CONFIG_HOME", None),
            ("HOME", Some(home_dir.to_str().expect("utf8 path"))),
            ("USERPROFILE", None),
        ]);
        let report = command
            .execute_direct_local(&observability, home_dir, current_dir)
            .expect("report");

        assert!(
            report
                .daemon_runtime
                .as_ref()
                .and_then(|runtime| runtime.peer_config.as_ref())
                .is_some(),
            "direct-local doctor must retain peer configuration visibility"
        );
        assert!(
            report.runtime_status.is_none(),
            "hermetic local doctor must not report live daemon runtime status"
        );
    }
}
