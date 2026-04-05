pub mod health;
pub mod report;

use std::path::PathBuf;

use crate::observability::ObservabilityPort;

pub use report::{
    DoctorEnvironmentVisibility, DoctorFinding, DoctorReport, DoctorSeverity, DoctorStatus,
    DoctorSummary,
};

#[derive(Debug, Clone)]
pub struct DoctorQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub team_override: Option<String>,
}

pub fn run_doctor(
    query: DoctorQuery,
    observability: &dyn ObservabilityPort,
) -> Result<DoctorReport, crate::error::AtmError> {
    let _ = &query.current_dir;

    let environment = health::environment_visibility(query.home_dir, query.team_override);
    let (observability_health, finding) = match observability.health() {
        Ok(health) => {
            let finding = health::observability_finding(&health);
            (health, finding)
        }
        Err(error) => {
            let snapshot = health::unavailable_snapshot(error.to_string());
            let finding = health::observability_finding_from_error(&error);
            (snapshot, finding)
        }
    };

    let findings = vec![finding];
    let recommendations = findings
        .iter()
        .filter_map(|finding| finding.remediation.clone())
        .collect::<Vec<_>>();
    let status = health::status_from_findings(&findings);
    let (info_count, warning_count, error_count) = findings.iter().fold(
        (0usize, 0usize, 0usize),
        |(info, warning, error), finding| match finding.severity {
            DoctorSeverity::Info => (info + 1, warning, error),
            DoctorSeverity::Warning => (info, warning + 1, error),
            DoctorSeverity::Error => (info, warning, error + 1),
        },
    );

    let message = match status {
        DoctorStatus::Healthy => "ATM doctor completed with healthy findings only",
        DoctorStatus::Warning => "ATM doctor completed with warnings",
        DoctorStatus::Error => "ATM doctor found critical issues",
    };

    Ok(DoctorReport {
        summary: DoctorSummary {
            status,
            message: message.to_string(),
            info_count,
            warning_count,
            error_count,
        },
        findings,
        recommendations,
        environment,
        observability: observability_health,
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::doctor::{DoctorQuery, DoctorSeverity, DoctorStatus, run_doctor};
    use crate::error::AtmError;
    use crate::error_codes::AtmErrorCode;
    use crate::observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        LogTailSession, ObservabilityPort,
    };

    enum StubHealth {
        Ok(AtmObservabilityHealth),
        Err(AtmError),
    }

    struct StubObservability {
        health: StubHealth,
    }

    impl ObservabilityPort for StubObservability {
        fn emit(&self, _event: crate::observability::CommandEvent) -> Result<(), AtmError> {
            Ok(())
        }

        fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
            Ok(AtmLogSnapshot::default())
        }

        fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
            Ok(LogTailSession::empty())
        }

        fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
            match &self.health {
                StubHealth::Ok(health) => Ok(health.clone()),
                StubHealth::Err(error) => Err(AtmError::new_with_code(
                    error.code,
                    error.kind,
                    error.message.clone(),
                )),
            }
        }
    }

    fn temp_path(name: impl AsRef<Path>) -> PathBuf {
        std::env::temp_dir()
            .join("atm-doctor-tests")
            .join(name.as_ref())
    }

    fn query() -> DoctorQuery {
        DoctorQuery {
            home_dir: temp_path("atm-home"),
            current_dir: temp_path("workspace"),
            team_override: Some("atm-dev".to_string()),
        }
    }

    #[test]
    fn run_doctor_reports_healthy_observability() {
        let report = run_doctor(
            query(),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(temp_path("atm.log.jsonl")),
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    detail: None,
                }),
            },
        )
        .expect("doctor report");

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
        assert_eq!(report.findings[0].severity, DoctorSeverity::Info);
        assert_eq!(report.findings[0].code, AtmErrorCode::ObservabilityHealthOk);
    }

    #[test]
    fn run_doctor_reports_degraded_observability_as_warning() {
        let report = run_doctor(
            query(),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(temp_path("atm.log.jsonl")),
                    logging_state: AtmObservabilityHealthState::Degraded,
                    query_state: Some(AtmObservabilityHealthState::Degraded),
                    detail: Some("query backlog".to_string()),
                }),
            },
        )
        .expect("doctor report");

        assert_eq!(report.summary.status, DoctorStatus::Warning);
        assert_eq!(report.findings[0].severity, DoctorSeverity::Warning);
        assert_eq!(
            report.findings[0].code,
            AtmErrorCode::WarningObservabilityHealthDegraded
        );
    }

    #[test]
    fn run_doctor_reports_unavailable_observability_as_error() {
        let report = run_doctor(
            query(),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: None,
                    logging_state: AtmObservabilityHealthState::Unavailable,
                    query_state: Some(AtmObservabilityHealthState::Unavailable),
                    detail: Some("logger unavailable".to_string()),
                }),
            },
        )
        .expect("doctor report");

        assert_eq!(report.summary.status, DoctorStatus::Error);
        assert_eq!(report.findings[0].severity, DoctorSeverity::Error);
        assert_eq!(
            report.findings[0].code,
            AtmErrorCode::ObservabilityHealthFailed
        );
    }

    #[test]
    fn run_doctor_reports_observability_health_errors() {
        let report = run_doctor(
            query(),
            &StubObservability {
                health: StubHealth::Err(AtmError::observability_health(
                    "health check transport failed",
                )),
            },
        )
        .expect("doctor report");

        assert_eq!(report.summary.status, DoctorStatus::Error);
        assert_eq!(report.findings[0].severity, DoctorSeverity::Error);
        assert_eq!(
            report.findings[0].code,
            AtmErrorCode::ObservabilityHealthFailed
        );
        assert_eq!(
            report.observability.logging_state,
            AtmObservabilityHealthState::Unavailable
        );
        assert!(
            report.findings[0]
                .message
                .contains("health check transport failed")
        );
    }
}
