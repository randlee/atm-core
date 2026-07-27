use crate::observability::CliObservability;
use crate::output;
use anyhow::Result;
use atm_core::doctor::{self, DaemonRuntimeDoctorReport, DoctorQuery, PeerLinkStatus};
use atm_daemon_bootstrap::assemble_default_runtime;
use clap::Args;

fn configured_peer_links(
    peer_config: &doctor::PeerConfigDoctorReport,
) -> Result<Vec<PeerLinkStatus>> {
    peer_config
        .trusted_peers
        .iter()
        .map(|peer| {
            peer.host
                .parse()
                .map(PeerLinkStatus::misconfigured)
                .map_err(anyhow::Error::from)
        })
        .collect()
}

use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};

#[derive(Debug, Args)]
/// Run ATM health and configuration diagnostics.
pub struct DoctorCommand {
    #[arg(long, help = "Override the resolved team for the doctor check.")]
    team: Option<String>,

    #[arg(long, help = "Emit the doctor report as JSON.")]
    json: bool,
}

impl DoctorCommand {
    // L.5 disposition (UNI-003): keep DoctorCommand injectability deferred for
    // initial release. Current service-level coverage exercises doctor behavior
    // without introducing a wider command abstraction before a concrete need
    // appears.
    /// Execute the `atm doctor` command.
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("doctor")?;
        let json = self.json;
        let report = self.execute(observability, home_dir, current_dir)?;

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
            caller_team,
            caller_identity,
        })
    }

    fn execute(
        self,
        observability: &CliObservability,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<atm_core::doctor::DoctorReport> {
        let local_report =
            self.execute_direct_local(observability, home_dir.clone(), current_dir.clone())?;
        let query = self.build_query(home_dir, current_dir)?;

        match CliComposition::bootstrap(
            "doctor",
            observability,
            InvocationDir::new(&query.current_dir),
            AtmHomePath::new(&query.home_dir),
        ) {
            Ok(composition) => composition.doctor(query).map_err(anyhow::Error::from),
            Err(_) => Ok(local_report),
        }
    }

    fn execute_direct_local(
        &self,
        observability: &CliObservability,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<atm_core::doctor::DoctorReport> {
        let query = self.build_query(home_dir, current_dir)?;
        let runtime = assemble_default_runtime()?;
        let (peer_config, peer_findings) =
            doctor::peer_config_doctor_report(runtime.peer_config_store().as_ref());
        let peer_links = configured_peer_links(&peer_config)?;
        doctor::run_doctor_with_runtime_ports(
            query,
            observability,
            &runtime.service_runtime,
            &runtime.doctor_ports,
            Some(DaemonRuntimeDoctorReport {
                findings: peer_findings,
                peer_config: Some(peer_config),
                peer_links,
                peer_wire_security: None,
            }),
        )
        .map_err(anyhow::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use atm_core::error::AtmError;
    use atm_core::test_support::EnvGuard;
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{DoctorCommand, configured_peer_links};
    use crate::observability::CliObservability;

    fn test_paths() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let current_dir = tempdir.path().join("cwd");
        (tempdir, home_dir, current_dir)
    }

    #[test]
    fn build_query_preserves_team_override() {
        let command = DoctorCommand {
            team: Some("test-team".to_string()),
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
    fn configured_peer_links_keeps_one_direct_local_row_per_peer() {
        let peer_config = atm_core::doctor::PeerConfigDoctorReport {
            trusted_peers: vec![atm_core::doctor::PeerAuthorityDoctorReport {
                host: "peer.example.test".to_string(),
                https_port: 43101,
                enabled: true,
            }],
            ..Default::default()
        };

        let links = configured_peer_links(&peer_config).expect("configured peer links");

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].peer.as_str(), "peer.example.test");
        assert_eq!(
            links[0].quality,
            atm_core::doctor::PeerLinkQuality::Misconfigured
        );
    }

    #[test]
    #[serial(env)]
    fn execute_runs_direct_local_doctor_path_without_inherited_bootstrap_configuration() {
        let observability = CliObservability::fallback();
        let command = DoctorCommand {
            team: None,
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
        let daemon_runtime = report
            .daemon_runtime
            .as_ref()
            .expect("direct-local daemon report");
        let peer_config = daemon_runtime
            .peer_config
            .as_ref()
            .expect("direct-local peer configuration");
        assert_eq!(
            daemon_runtime.peer_links.len(),
            peer_config.trusted_peers.len()
        );
        assert!(daemon_runtime.peer_links.iter().all(|link| {
            link.quality == atm_core::doctor::PeerLinkQuality::Misconfigured
                && peer_config
                    .trusted_peers
                    .iter()
                    .any(|peer| peer.host == link.peer.as_str())
        }));
        assert!(
            report.runtime_status.is_none(),
            "hermetic local doctor must not report live daemon runtime status"
        );
    }
}
