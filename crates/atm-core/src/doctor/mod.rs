pub mod health;
pub mod report;

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::config;
use crate::error_codes::AtmErrorCode;
use crate::observability::ObservabilityPort;
use crate::schema::AgentMember;
use crate::team_admin::{MemberSummary, MembersList};

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
    let config = config::load_config(&query.current_dir)?;
    let home_dir = query.home_dir.clone();
    let resolved_team = config::resolve_team(query.team_override.as_deref(), config.as_ref());

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

    let mut findings = Vec::new();
    if config
        .as_ref()
        .is_some_and(|config| config.obsolete_identity_present)
    {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningIdentityDrift,
            message: "obsolete [atm].identity is still present in .atm.toml; ATM no longer uses config identity as a runtime fallback.".to_string(),
            remediation: Some(
                "Remove [atm].identity from .atm.toml and set ATM_IDENTITY in the active agent environment instead."
                    .to_string(),
            ),
        });
    }
    let member_roster = resolved_team
        .as_deref()
        .and_then(|team| load_member_roster(&home_dir, team, config.as_ref(), &mut findings));
    findings.push(finding);
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
        member_roster,
        observability: observability_health,
    })
}

fn load_member_roster(
    home_dir: &std::path::Path,
    team: &str,
    config: Option<&config::AtmConfig>,
    findings: &mut Vec<DoctorFinding>,
) -> Option<MembersList> {
    let team_dir = crate::home::team_dir_from_home(home_dir, team).ok()?;
    let team_config = config::load_team_config(&team_dir).ok()?;
    let baseline = config
        .map(|config| config.team_members.as_slice())
        .unwrap_or(&[]);

    let present = team_config
        .members
        .iter()
        .map(|member| member.name.clone())
        .collect::<BTreeSet<_>>();
    for member in baseline {
        if present.contains(member) {
            continue;
        }
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningBaselineMemberMissing,
            message: format!(
                "baseline member '{member}' is missing from team config.json for '{team}'"
            ),
            remediation: Some(format!(
                "Restore '{member}' in .claude/teams/{team}/config.json or remove it from [atm].team_members if it is no longer part of the baseline roster."
            )),
        });
    }

    Some(MembersList {
        team: team.to_string(),
        members: ordered_member_summaries(&team_config.members, baseline),
    })
}

fn ordered_member_summaries(members: &[AgentMember], baseline: &[String]) -> Vec<MemberSummary> {
    let mut ordered = Vec::new();
    let mut included = BTreeSet::new();

    if baseline.iter().any(|member| member == "team-lead")
        && let Some(team_lead) = members.iter().find(|member| member.name == "team-lead")
    {
        ordered.push(member_summary(team_lead));
        included.insert(team_lead.name.clone());
    }

    for baseline_member in baseline {
        if baseline_member == "team-lead" {
            continue;
        }
        if let Some(member) = members
            .iter()
            .find(|member| member.name == *baseline_member)
        {
            ordered.push(member_summary(member));
            included.insert(member.name.clone());
        }
    }

    for member in members {
        if included.insert(member.name.clone()) {
            ordered.push(member_summary(member));
        }
    }

    ordered
}

fn member_summary(member: &AgentMember) -> MemberSummary {
    MemberSummary {
        name: member.name.clone(),
        agent_id: member.agent_id.clone(),
        agent_type: member.agent_type.clone(),
        model: member.model.clone(),
        joined_at: member.joined_at,
        tmux_pane_id: member.tmux_pane_id.clone(),
        cwd: member.cwd.clone(),
        extra: member.extra.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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

    impl crate::observability::sealed::Sealed for StubObservability {}

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

    struct TestPaths {
        _tempdir: tempfile::TempDir,
        home_dir: PathBuf,
        current_dir: PathBuf,
        active_log_path: PathBuf,
    }

    impl TestPaths {
        fn new() -> Self {
            let tempdir = tempfile::tempdir().expect("tempdir");
            let root = tempdir.path().to_path_buf();
            let home_dir = root.join("atm-home");
            let current_dir = root.join("workspace");
            std::fs::create_dir_all(&home_dir).expect("home dir");
            std::fs::create_dir_all(&current_dir).expect("workspace dir");
            Self {
                _tempdir: tempdir,
                home_dir,
                current_dir,
                active_log_path: root.join("atm.log.jsonl"),
            }
        }
    }

    fn query(paths: &TestPaths) -> DoctorQuery {
        DoctorQuery {
            home_dir: paths.home_dir.clone(),
            current_dir: paths.current_dir.clone(),
            team_override: Some("atm-dev".to_string()),
        }
    }

    #[test]
    fn run_doctor_reports_healthy_observability() {
        let paths = TestPaths::new();
        let report = run_doctor(
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
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
    fn run_doctor_reports_obsolete_identity_drift_as_warning() {
        let paths = TestPaths::new();
        std::fs::write(
            paths.current_dir.join(".atm.toml"),
            "[atm]\nidentity = \"arch-ctm\"\n",
        )
        .expect("config");
        let report = run_doctor(
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    detail: None,
                }),
            },
        )
        .expect("doctor report");

        assert_eq!(report.summary.status, DoctorStatus::Warning);
        assert_eq!(report.findings[0].severity, DoctorSeverity::Warning);
        assert_eq!(report.findings[0].code, AtmErrorCode::WarningIdentityDrift);
        assert!(
            report.findings[0]
                .message
                .contains("obsolete [atm].identity")
        );
        assert_eq!(report.findings[1].code, AtmErrorCode::ObservabilityHealthOk);
    }

    #[test]
    fn run_doctor_reports_degraded_observability_as_warning() {
        let paths = TestPaths::new();
        let report = run_doctor(
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
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
        let paths = TestPaths::new();
        let report = run_doctor(
            query(&paths),
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
        let paths = TestPaths::new();
        let report = run_doctor(
            query(&paths),
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
