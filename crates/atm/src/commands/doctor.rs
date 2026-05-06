use anyhow::Result;
use atm_core::doctor::DoctorQuery;
use atm_core::home;
use clap::Args;

use crate::composition::CliComposition;
use crate::observability::CliObservability;
use crate::output;

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
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let composition = CliComposition::bootstrap(observability)?;
        let json = self.json;
        let report = self.execute(&composition, home_dir, current_dir)?;

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
        Ok(DoctorQuery {
            home_dir,
            current_dir,
            team_override: self.team.as_ref().map(|value| value.parse()).transpose()?,
        })
    }

    fn execute(
        self,
        composition: &CliComposition,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<atm_core::doctor::DoctorReport> {
        composition
            .doctor(self.build_query(home_dir, current_dir)?)
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use atm_core::doctor::{
        DoctorEnvironmentVisibility, DoctorFinding, DoctorReport, DoctorStatus, DoctorSummary,
    };
    use atm_core::error::AtmError;
    use atm_core::observability::{AtmObservabilityHealth, AtmObservabilityHealthState};
    use atm_core::protocol::{ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope};
    use atm_core::transport::testing::FakeClientTransport;

    use super::DoctorCommand;
    use crate::composition::CliComposition;
    use crate::observability::CliObservability;

    fn healthy_report() -> DoctorReport {
        DoctorReport {
            summary: DoctorSummary {
                status: DoctorStatus::Healthy,
                message: "ok".to_string(),
                info_count: 0,
                warning_count: 0,
                error_count: 0,
            },
            findings: Vec::<DoctorFinding>::new(),
            recommendations: Vec::new(),
            environment: DoctorEnvironmentVisibility {
                atm_home: None,
                atm_team: None,
                atm_identity: None,
                team_override: None,
            },
            member_roster: None,
            observability: AtmObservabilityHealth {
                active_log_path: None,
                logging_state: AtmObservabilityHealthState::Healthy,
                query_state: Some(AtmObservabilityHealthState::Healthy),
                detail: None,
            },
        }
    }

    #[test]
    fn build_query_preserves_team_override() {
        let command = DoctorCommand {
            team: Some("test-team".to_string()),
            json: true,
        };

        let query = command
            .build_query(PathBuf::from("/tmp/home"), PathBuf::from("/tmp/cwd"))
            .expect("query");

        assert_eq!(
            query.team_override.as_ref().map(|value| value.as_str()),
            Some("test-team")
        );
    }

    #[test]
    fn execute_accepts_fake_transport_healthy_report() {
        let transport = Arc::new(FakeClientTransport::new(|request| match request {
            RequestEnvelope::Doctor(_) => Ok(ResponseEnvelope::Doctor(healthy_report())),
            other => panic!("unexpected request: {other:?}"),
        }));
        let observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(transport, &observability);
        let command = DoctorCommand {
            team: Some("test-team".to_string()),
            json: false,
        };

        let report = command
            .execute(
                &composition,
                PathBuf::from("/tmp/home"),
                PathBuf::from("/tmp/cwd"),
            )
            .expect("report");

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
    }

    #[test]
    fn execute_surfaces_fake_transport_doctor_error() {
        let transport = Arc::new(FakeClientTransport::new(|request| match request {
            RequestEnvelope::Doctor(_) => {
                Ok(ResponseEnvelope::Error(ProtocolErrorEnvelope::from_error(
                    &AtmError::daemon_unavailable("synthetic doctor transport failure"),
                )))
            }
            other => panic!("unexpected request: {other:?}"),
        }));
        let observability = CliObservability::fallback();
        let composition = CliComposition::from_transport(transport, &observability);
        let command = DoctorCommand {
            team: None,
            json: false,
        };

        let error = command
            .execute(
                &composition,
                PathBuf::from("/tmp/home"),
                PathBuf::from("/tmp/cwd"),
            )
            .expect_err("doctor error");

        let atm_error = error
            .downcast_ref::<AtmError>()
            .expect("doctor error should preserve AtmError");
        assert_eq!(
            atm_error.code,
            atm_core::error_codes::AtmErrorCode::DaemonUnavailable
        );
        assert!(
            error
                .to_string()
                .contains("synthetic doctor transport failure")
        );
    }
}
