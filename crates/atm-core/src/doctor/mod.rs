pub mod health;
pub mod report;

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;
use crate::error_codes::AtmErrorCode;
use crate::observability::ObservabilityPort;
use crate::schema::AgentMember;
use crate::team_admin::{MemberSummary, MembersList};
use crate::types::{AgentName, TeamName};
use serde::{Deserialize, Serialize};

pub use report::{
    DoctorEnvironmentVisibility, DoctorFinding, DoctorReport, DoctorRuntimeHealth, DoctorSeverity,
    DoctorStatus, DoctorSummary,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub team_override: Option<TeamName>,
}

/// Run the ATM doctor checks for config, roster, and observability health.
///
/// # Errors
///
/// Returns [`crate::error::AtmError`] when loading `.atm.toml` fails before the
/// doctor report can be assembled.
pub fn run_doctor(
    query: DoctorQuery,
    observability: &dyn ObservabilityPort,
) -> Result<DoctorReport, crate::error::AtmError> {
    let config = config::load_config(&query.current_dir)?;
    let home_dir = query.home_dir.clone();
    let initial_lock_snapshot = snapshot_mailbox_lock_paths(&home_dir);
    let resolved_team = query
        .team_override
        .clone()
        .or_else(|| config::resolve_team(None, config.as_ref()));

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
    push_stale_mailbox_lock_findings(
        &initial_lock_snapshot,
        &snapshot_mailbox_lock_paths(&home_dir),
        &mut findings,
    );
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
        runtime: None,
    })
}

fn load_member_roster(
    home_dir: &Path,
    team: &str,
    config: Option<&config::AtmConfig>,
    findings: &mut Vec<DoctorFinding>,
) -> Option<MembersList> {
    let team_dir = match crate::home::team_dir_from_home(home_dir, team) {
        Ok(team_dir) => team_dir,
        Err(error) => {
            push_doctor_error(findings, DoctorSeverity::Error, error);
            return None;
        }
    };
    if !team_dir.is_dir() {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Error,
            code: AtmErrorCode::TeamNotFound,
            message: format!(
                "team directory is missing at {} for '{}'",
                team_dir.display(),
                team
            ),
            remediation: Some(format!(
                "Create .claude/teams/{team} or correct ATM_HOME / --team before rerunning `atm doctor`."
            )),
        });
        return None;
    }

    check_restore_marker(team, &team_dir, findings);

    let team_config = match config::load_team_config(&team_dir) {
        Ok(team_config) => team_config,
        Err(error) => {
            push_doctor_error(findings, DoctorSeverity::Error, error);
            return None;
        }
    };
    let baseline = config
        .map(|config| config.team_members.as_slice())
        .unwrap_or(&[]);

    check_inbox_directory(team, &team_dir.join("inboxes"), findings);

    let present = team_config
        .members
        .iter()
        .map(|member| member.name.clone())
        .collect::<BTreeSet<_>>();
    for member in baseline {
        if present
            .iter()
            .any(|present_member| present_member == &member.as_str())
        {
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
        team: TeamName::from_validated(team.to_string()),
        members: ordered_member_summaries(&team_config.members, baseline),
    })
}

fn push_doctor_error(
    findings: &mut Vec<DoctorFinding>,
    severity: DoctorSeverity,
    error: crate::error::AtmError,
) {
    findings.push(DoctorFinding {
        severity,
        code: error.code,
        message: error.message,
        remediation: error.recovery,
    });
}

fn check_inbox_directory(team: &str, inboxes_dir: &Path, findings: &mut Vec<DoctorFinding>) {
    if !inboxes_dir.is_dir() {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Error,
            code: AtmErrorCode::MailboxWriteFailed,
            message: format!(
                "inbox directory is missing at {} for '{}'",
                inboxes_dir.display(),
                team
            ),
            remediation: Some(format!(
                "Create .claude/teams/{team}/inboxes and ensure ATM can write inbox files before rerunning `atm doctor`."
            )),
        });
        return;
    }

    if let Err(error) = probe_directory_writable(inboxes_dir) {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Error,
            code: AtmErrorCode::MailboxWriteFailed,
            message: format!(
                "inbox directory is not writable at {}: {error}",
                inboxes_dir.display()
            ),
            remediation: Some(
                "Check inbox directory permissions and ensure ATM can create and remove inbox files before rerunning `atm doctor`."
                    .to_string(),
            ),
        });
    }
}

fn check_restore_marker(team: &str, team_dir: &Path, findings: &mut Vec<DoctorFinding>) {
    let marker = team_dir.join(".restore-in-progress");
    if !marker.is_file() {
        return;
    }

    findings.push(DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: AtmErrorCode::WarningRestoreInProgress,
        message: format!(
            "stale restore marker is present at {} for '{}'; a prior `atm teams restore` may have been interrupted",
            marker.display(),
            team
        ),
        remediation: Some(format!(
            "Inspect {} for partial restore state, rerun `atm teams restore {team}`, then remove the marker once recovery is complete.",
            team_dir.display()
        )),
    });
}

fn snapshot_mailbox_lock_paths(home_dir: &Path) -> BTreeSet<PathBuf> {
    let teams_root = home_dir.join(".claude").join("teams");
    let Ok(team_entries) = fs::read_dir(&teams_root) else {
        return BTreeSet::new();
    };

    let mut locks = BTreeSet::new();
    for team_entry in team_entries.filter_map(Result::ok) {
        let inboxes_dir = team_entry.path().join("inboxes");
        let Ok(lock_entries) = fs::read_dir(inboxes_dir) else {
            continue;
        };
        for lock_entry in lock_entries.filter_map(Result::ok) {
            let path = lock_entry.path();
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !(file_name.ends_with(".lock") || file_name.contains(".lock.")) {
                continue;
            }
            if !lock_entry
                .file_type()
                .is_ok_and(|file_type| file_type.is_file())
            {
                continue;
            }
            locks.insert(path);
        }
    }

    locks
}

fn push_stale_mailbox_lock_findings(
    initial: &BTreeSet<PathBuf>,
    final_snapshot: &BTreeSet<PathBuf>,
    findings: &mut Vec<DoctorFinding>,
) {
    for path in initial.intersection(final_snapshot) {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningStaleMailboxLock,
            message: format!(
                "mailbox lock sentinel persisted for the full doctor run at {}; the lock is likely stale",
                path.display()
            ),
            remediation: Some(format!(
                "Confirm no live ATM process still owns the mailbox, then remove the stale sentinel with `rm -f {}`.",
                path.display()
            )),
        });
    }
}

fn probe_directory_writable(directory: &Path) -> Result<(), std::io::Error> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let probe_path = directory.join(format!(
        ".atm-doctor-write-probe-{}-{nonce}",
        std::process::id()
    ));
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe_path)?;
    drop(file);
    fs::remove_file(&probe_path)?;
    Ok(())
}

fn ordered_member_summaries(members: &[AgentMember], baseline: &[TeamName]) -> Vec<MemberSummary> {
    let mut ordered = Vec::new();
    let mut included = BTreeSet::new();

    if baseline.iter().any(|member| member.as_str() == "team-lead")
        && let Some(team_lead) = members.iter().find(|member| member.name == "team-lead")
    {
        ordered.push(member_summary(team_lead));
        included.insert(team_lead.name.clone());
    }

    for baseline_member in baseline {
        if baseline_member.as_str() == "team-lead" {
            continue;
        }
        if let Some(member) = members
            .iter()
            .find(|member| member.name == baseline_member.as_str())
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
        name: AgentName::from_validated(member.name.clone()),
        agent_id: member.agent_id.clone().unwrap_or_default(),
        agent_type: member
            .agent_type
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        model: member.model.clone().unwrap_or_default(),
        joined_at: member.joined_at,
        tmux_pane_id: member.tmux_pane_id.clone().unwrap_or_default(),
        cwd: member.cwd.clone().unwrap_or_default(),
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
    use crate::schema::{AgentMember, TeamConfig};
    use crate::types::AgentName;

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

        fn team_dir(&self) -> PathBuf {
            self.home_dir.join(".claude").join("teams").join("atm-dev")
        }

        fn write_team_layout(&self, members: &[&str]) {
            let team_dir = self.team_dir();
            std::fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes dir");
            let config = TeamConfig {
                members: members
                    .iter()
                    .map(|member| AgentMember::with_name(AgentName::from_validated(*member)))
                    .collect(),
                ..Default::default()
            };
            std::fs::write(
                team_dir.join("config.json"),
                serde_json::to_vec(&config).expect("team config"),
            )
            .expect("write team config");
        }

        fn write_raw_team_config(&self, raw: &str) {
            let team_dir = self.team_dir();
            std::fs::create_dir_all(&team_dir).expect("team dir");
            std::fs::write(team_dir.join("config.json"), raw).expect("write raw team config");
        }
    }

    fn query(paths: &TestPaths) -> DoctorQuery {
        DoctorQuery {
            home_dir: paths.home_dir.clone(),
            current_dir: paths.current_dir.clone(),
            team_override: Some("atm-dev".parse().expect("team")),
        }
    }

    #[test]
    fn run_doctor_reports_healthy_observability() {
        let paths = TestPaths::new();
        paths.write_team_layout(&["arch-ctm"]);
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
    fn run_doctor_reports_invalid_team_override_as_address_error() {
        let paths = TestPaths::new();
        let report = run_doctor(
            DoctorQuery {
                home_dir: paths.home_dir.clone(),
                current_dir: paths.current_dir.clone(),
                team_override: Some(crate::types::TeamName::from_validated("../evil")),
            },
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

        assert!(report.member_roster.is_none());
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == AtmErrorCode::AddressParseFailed),
            "{report:#?}"
        );
    }

    #[test]
    fn run_doctor_reports_obsolete_identity_drift_as_warning() {
        let paths = TestPaths::new();
        paths.write_team_layout(&["arch-ctm"]);
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
        paths.write_team_layout(&["arch-ctm"]);
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
        paths.write_team_layout(&["arch-ctm"]);
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
        paths.write_team_layout(&["arch-ctm"]);
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

    #[test]
    fn run_doctor_reports_missing_team_directory_as_error() {
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

        assert_eq!(report.summary.status, DoctorStatus::Error);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == AtmErrorCode::TeamNotFound),
            "{report:#?}"
        );
    }

    #[test]
    fn run_doctor_reports_team_config_parse_failure_as_error() {
        let paths = TestPaths::new();
        paths.write_raw_team_config("{\"members\":");
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

        assert_eq!(report.summary.status, DoctorStatus::Error);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == AtmErrorCode::ConfigTeamParseFailed),
            "{report:#?}"
        );
    }

    #[test]
    fn run_doctor_reports_missing_inboxes_directory_as_error() {
        let paths = TestPaths::new();
        paths.write_raw_team_config(r#"{"members":[{"name":"arch-ctm"}]}"#);
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

        assert_eq!(report.summary.status, DoctorStatus::Error);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == AtmErrorCode::MailboxWriteFailed),
            "{report:#?}"
        );
    }

    #[test]
    fn run_doctor_reports_stale_mailbox_lock_as_warning() {
        let paths = TestPaths::new();
        paths.write_team_layout(&["arch-ctm"]);
        let stale_lock = paths.team_dir().join("inboxes").join("arch-ctm.json.lock");
        std::fs::write(&stale_lock, u32::MAX.to_string()).expect("stale lock");
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
        assert!(
            report.findings.iter().any(|finding| {
                finding.code == AtmErrorCode::WarningStaleMailboxLock
                    && finding.message.contains(&stale_lock.display().to_string())
            }),
            "{report:#?}"
        );
    }
}
