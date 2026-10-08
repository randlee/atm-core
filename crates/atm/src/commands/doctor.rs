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
    fn unique_name_f06_all_teams_does_not_widen_pane_alias_scope() {
        let config = pane_config(
            r#"[[rmux.windows.panes]]
name = "member-a"
alias = "wrong-workspace-alias"
env = { ATM_TEAM = "workspace" }

[[rmux.windows.panes]]
name = "member-b"
alias = "wrong-other-alias"
env = { ATM_TEAM = "other" }
"#,
        );

        // Even when the service report contains every roster (`--all-teams`),
        // this CLI-only .atm.toml check is anchored to the invoking workspace.
        let mismatches = pane_alias_mismatches(
            config,
            &workspace_team(),
            &alias_map(&[("member-a", Some("workspace-alias")), ("member-b", None)]),
        );

        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].team, workspace_team());
        assert_eq!(mismatches[0].member.as_str(), "member-a");
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

    const EXPORT_JSON_CHILD: &str = "commands::doctor::tests::doctor_json_export_health_child";
    const EXPORT_JSON_STATE: &str = "ATM_DOCTOR_EXPORT_JSON_STATE";
    const JSON_BEGIN: &str = "--- atm doctor --json begin ---";
    const JSON_END: &str = "--- atm doctor --json end ---";

    /// The daemon's observability port as doctor reads it: healthy logging
    /// with the given OpenTelemetry export health, if any.
    struct ExportHealthObservability(Option<atm_core::observability::AtmTelemetryExportHealth>);

    impl atm_core::boundary::sealed::Sealed for ExportHealthObservability {}

    impl atm_core::observability::ObservabilityPort for ExportHealthObservability {
        fn emit(&self, _event: atm_core::observability::CommandEvent) -> Result<(), AtmError> {
            Ok(())
        }

        fn query(
            &self,
            _req: atm_core::observability::AtmLogQuery,
        ) -> Result<atm_core::observability::AtmLogSnapshot, AtmError> {
            Ok(Default::default())
        }

        fn follow(
            &self,
            _req: atm_core::observability::AtmLogQuery,
        ) -> Result<atm_core::observability::LogTailSession, AtmError> {
            Ok(atm_core::observability::LogTailSession::empty())
        }

        fn health(&self) -> Result<atm_core::observability::AtmObservabilityHealth, AtmError> {
            let mut health = atm_core::transport::testing::HealthyObservability.health()?;
            health.export = self.0.clone();
            Ok(health)
        }
    }

    fn export_health(state: &str) -> Option<atm_core::observability::AtmTelemetryExportHealth> {
        use atm_core::observability::{
            AtmTelemetryExportFailure, AtmTelemetryExportHealth, AtmTelemetryExportState,
        };
        match state {
            // An invalid ATM_OTEL_ENDPOINT, as the daemon classifies it.
            "unavailable" => Some(AtmTelemetryExportHealth {
                state: AtmTelemetryExportState::Unavailable,
                endpoint: None,
                protocol: None,
                emitted: 0,
                dropped_full: 0,
                dropped_timeout: 0,
                dropped_failure: 0,
                dropped_shutdown: 0,
                last_failure: Some(AtmTelemetryExportFailure::ConfigInvalid),
            }),
            // A configured collector that refused one batch.
            "degraded" => Some(AtmTelemetryExportHealth {
                state: AtmTelemetryExportState::Degraded,
                endpoint: Some("http://127.0.0.1:4317".to_owned()),
                protocol: Some(atm_core::task_telemetry::TelemetryExportProtocol::Grpc),
                emitted: 3,
                dropped_full: 0,
                dropped_timeout: 0,
                dropped_failure: 2,
                dropped_shutdown: 0,
                last_failure: Some(AtmTelemetryExportFailure::Unavailable),
            }),
            // No ATM_OTEL_ENDPOINT: the daemon reports no export block.
            "none" => None,
            other => panic!("unknown export state {other}"),
        }
    }

    /// Child half of [`doctor_json_reports_unavailable_and_degraded_export`]:
    /// the CLI's daemon-routed doctor request, its pane-alias merge and
    /// `print_doctor_result(.., json = true)` on this process's real stdout.
    #[test]
    #[serial(env)]
    fn doctor_json_export_health_child() {
        let Ok(state) = std::env::var(EXPORT_JSON_STATE) else {
            return;
        };
        let fixture =
            crate::composition::tests::LoopbackFixture::new(atm_core::test_support::TEST_RECIPIENT);
        let transport = atm_core::transport::testing::LoopbackClientTransport::new(
            std::sync::Arc::new(ExportHealthObservability(export_health(&state))),
        );
        let observability = CliObservability::fallback();
        let composition = crate::composition::CliComposition::from_loopback_transport(
            std::sync::Arc::new(transport),
            &observability,
        );
        let query = atm_core::doctor::DoctorQuery {
            home_dir: fixture.home_dir.clone(),
            current_dir: fixture.current_dir.clone(),
            team_override: Some(atm_core::test_support::TEST_TEAM.parse().expect("team")),
            ..atm_core::doctor::DoctorQuery::default()
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("child runtime");
        let mut report = runtime
            .block_on(composition.doctor(query.clone()))
            .expect("daemon-routed doctor report");
        super::append_pane_alias_mismatches(&mut report, &query);
        println!("\n{JSON_BEGIN}");
        crate::output::print_doctor_result(&report, true).expect("doctor --json output");
        println!("{JSON_END}");
    }

    /// Runs the child in its own process and parses the JSON it printed.
    fn doctor_json_stdout(state: &str) -> serde_json::Value {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                EXPORT_JSON_CHILD,
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(EXPORT_JSON_STATE, state)
            .output()
            .expect("run the doctor --json child");
        let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
        assert!(
            output.status.success(),
            "doctor --json child failed\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let json = stdout
            .split_once(JSON_BEGIN)
            .and_then(|(_, rest)| rest.split_once(JSON_END))
            .map(|(json, _)| json)
            .unwrap_or_else(|| panic!("no doctor --json block in child stdout:\n{stdout}"));
        serde_json::from_str(json).unwrap_or_else(|error| panic!("{error}: {json}"))
    }

    fn observability_finding(report: &serde_json::Value) -> &serde_json::Value {
        report["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .find(|finding| {
                finding["message"]
                    .as_str()
                    .is_some_and(|message| message.starts_with("shared observability"))
            })
            .unwrap_or_else(|| panic!("no observability finding: {report:#}"))
    }

    /// Positive: `atm doctor --json` stdout carries the daemon's Unavailable
    /// and Degraded export health with its counters and last failure, and the
    /// observability finding becomes a warning naming ATM_OTEL_ENDPOINT.
    /// Negative: with no export configured the stdout has no export block and
    /// the finding stays informational.
    #[test]
    fn doctor_json_reports_unavailable_and_degraded_export() {
        let unavailable = doctor_json_stdout("unavailable");
        let export = &unavailable["observability"]["export"];
        assert_eq!(export["state"], "unavailable", "{unavailable:#}");
        assert_eq!(export["last_failure"], "config_invalid");
        assert_eq!(export["endpoint"], serde_json::Value::Null);
        let finding = observability_finding(&unavailable);
        assert_eq!(finding["severity"], "warning", "{finding:#}");
        assert!(
            finding["remediation"]
                .as_str()
                .is_some_and(|text| text.contains("ATM_OTEL_ENDPOINT")),
            "{finding:#}"
        );

        let degraded = doctor_json_stdout("degraded");
        let export = &degraded["observability"]["export"];
        assert_eq!(export["state"], "degraded", "{degraded:#}");
        assert_eq!(export["endpoint"], "http://127.0.0.1:4317");
        assert_eq!(export["protocol"], "grpc");
        assert_eq!(export["emitted"], 3);
        assert_eq!(export["dropped_failure"], 2);
        assert_eq!(export["last_failure"], "unavailable");
        assert_eq!(observability_finding(&degraded)["severity"], "warning");

        let unconfigured = doctor_json_stdout("none");
        assert!(
            unconfigured["observability"].get("export").is_none(),
            "{unconfigured:#}"
        );
        assert_eq!(observability_finding(&unconfigured)["severity"], "info");
    }
}
