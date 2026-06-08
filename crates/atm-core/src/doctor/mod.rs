pub mod health;
pub mod report;

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::boundary::{ConfigDoctor, MailStoreDoctor, RosterMemberRecord, RosterStoreDoctor};
use crate::config;
use crate::error_codes::AtmErrorCode;
use crate::observability::ObservabilityPort;
use crate::roles::ROLE_TEAM_LEAD;
use crate::schema::AgentMember;
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::default_runtime;
use crate::team_admin::{MemberSummary, MembersList};
use crate::types::{AgentName, TeamName};
use std::sync::Arc;

pub use report::{
    BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
    BootstrapTraceReport, DaemonRuntimeDoctorReport, DoctorEnvironmentVisibility, DoctorFinding,
    DoctorReport, DoctorSeverity, DoctorStatus, DoctorSummary,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DoctorQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub team_override: Option<TeamName>,
}

#[derive(Clone)]
pub struct RuntimeDoctorPorts {
    pub config_doctor: Arc<dyn ConfigDoctor + Send + Sync>,
    pub mail_store_doctor: Arc<dyn MailStoreDoctor + Send + Sync>,
    pub roster_store_doctor: Arc<dyn RosterStoreDoctor + Send + Sync>,
}

impl std::fmt::Debug for RuntimeDoctorPorts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeDoctorPorts")
            .field("config_doctor", &"dyn ConfigDoctor")
            .field("mail_store_doctor", &"dyn MailStoreDoctor")
            .field("roster_store_doctor", &"dyn RosterStoreDoctor")
            .finish()
    }
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
    let runtime = default_runtime()?;
    run_doctor_with_runtime(query, observability, &runtime)
}

pub fn run_doctor_with_runtime(
    query: DoctorQuery,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<DoctorReport, crate::error::AtmError> {
    let config = runtime.load_config(&query.current_dir)?;
    let home_dir = query.home_dir.clone();
    let initial_lock_snapshot = snapshot_mailbox_lock_paths(&home_dir);
    let resolved_team = resolved_doctor_team(&query, config.as_ref());
    let environment = health::environment_visibility(query.home_dir, query.team_override);
    let (observability_health, finding) = doctor_observability_status(observability);
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
    let member_roster = resolved_team.as_ref().and_then(|team| {
        load_member_roster(runtime, &home_dir, team, config.as_ref(), &mut findings)
    });
    push_stale_mailbox_lock_findings(
        &initial_lock_snapshot,
        &snapshot_mailbox_lock_paths(&home_dir),
        &mut findings,
    );
    findings.push(finding);
    let summary = summarize_doctor_findings(&findings);
    let recommendations = findings
        .iter()
        .filter_map(|finding| finding.remediation.clone())
        .collect::<Vec<_>>();

    Ok(DoctorReport {
        summary,
        findings,
        recommendations,
        environment,
        member_roster,
        observability: observability_health,
        config: crate::boundary::ConfigDoctorReport::default(),
        mail_store: crate::boundary::MailStoreDoctorReport::default(),
        roster_store: crate::boundary::RosterStoreDoctorReport::default(),
        daemon_runtime: None,
        drift_findings: Vec::new(),
        runtime_status: None,
        bootstrap_trace: None,
    })
}

pub fn run_doctor_with_runtime_ports(
    query: DoctorQuery,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
    runtime_doctors: &RuntimeDoctorPorts,
    daemon_runtime: Option<report::DaemonRuntimeDoctorReport>,
) -> Result<DoctorReport, crate::error::AtmError> {
    let config = runtime.load_config(&query.current_dir)?;
    let home_dir = query.home_dir.clone();
    let initial_lock_snapshot = snapshot_mailbox_lock_paths(&home_dir);
    let resolved_team = resolved_doctor_team(&query, config.as_ref());
    let environment = health::environment_visibility(query.home_dir, query.team_override);
    let (observability_health, observability_finding) = doctor_observability_status(observability);
    let mut general_findings = Vec::new();
    let mut drift_findings = Vec::new();
    let mut reports = inspect_runtime_doctor_sections(runtime_doctors, &mut general_findings);
    push_obsolete_identity_finding(config.as_ref(), &mut reports.config);
    let member_roster = resolved_team.as_ref().and_then(|team| {
        load_member_roster(
            runtime,
            &home_dir,
            team,
            config.as_ref(),
            &mut drift_findings,
        )
    });
    push_stale_mailbox_lock_findings(
        &initial_lock_snapshot,
        &snapshot_mailbox_lock_paths(&home_dir),
        &mut drift_findings,
    );
    let findings = collect_doctor_findings(
        &reports,
        &drift_findings,
        &general_findings,
        observability_finding,
        daemon_runtime.as_ref(),
    );
    let summary = summarize_doctor_findings(&findings);
    let recommendations = collect_recommendations(&findings);

    Ok(DoctorReport {
        summary,
        findings,
        recommendations,
        environment,
        member_roster,
        observability: observability_health,
        config: reports.config,
        mail_store: reports.mail_store,
        roster_store: reports.roster_store,
        daemon_runtime,
        drift_findings,
        runtime_status: None,
        bootstrap_trace: None,
    })
}

struct DoctorSectionReports {
    config: crate::boundary::ConfigDoctorReport,
    mail_store: crate::boundary::MailStoreDoctorReport,
    roster_store: crate::boundary::RosterStoreDoctorReport,
}

fn inspect_runtime_doctor_sections(
    runtime_doctors: &RuntimeDoctorPorts,
    findings: &mut Vec<DoctorFinding>,
) -> DoctorSectionReports {
    DoctorSectionReports {
        config: inspect_doctor_section(runtime_doctors.config_doctor.inspect_config(), findings),
        mail_store: inspect_doctor_section(
            runtime_doctors.mail_store_doctor.inspect_mail_store(),
            findings,
        ),
        roster_store: inspect_doctor_section(
            runtime_doctors.roster_store_doctor.inspect_roster_store(),
            findings,
        ),
    }
}

fn push_obsolete_identity_finding(
    config: Option<&config::AtmConfig>,
    config_report: &mut crate::boundary::ConfigDoctorReport,
) {
    if config.is_some_and(|config| config.obsolete_identity_present) {
        config_report.findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningIdentityDrift,
            message: "obsolete [atm].identity is still present in .atm.toml; ATM no longer uses config identity as a runtime fallback.".to_string(),
            remediation: Some(
                "Remove [atm].identity from .atm.toml and set ATM_IDENTITY in the active agent environment instead."
                    .to_string(),
            ),
        });
    }
}

fn collect_doctor_findings(
    reports: &DoctorSectionReports,
    drift_findings: &[DoctorFinding],
    general_findings: &[DoctorFinding],
    observability_finding: DoctorFinding,
    daemon_runtime: Option<&report::DaemonRuntimeDoctorReport>,
) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();
    findings.extend(reports.config.findings.iter().cloned());
    findings.extend(reports.mail_store.findings.iter().cloned());
    findings.extend(reports.roster_store.findings.iter().cloned());
    findings.extend(drift_findings.iter().cloned());
    findings.extend(general_findings.iter().cloned());
    findings.push(observability_finding);
    if let Some(runtime_report) = daemon_runtime {
        findings.extend(runtime_report.findings.iter().cloned());
    }
    findings
}

fn collect_recommendations(findings: &[DoctorFinding]) -> Vec<String> {
    findings
        .iter()
        .filter_map(|finding| finding.remediation.clone())
        .collect()
}

fn inspect_doctor_section<T>(
    result: Result<T, crate::error::AtmError>,
    findings: &mut Vec<DoctorFinding>,
) -> T
where
    T: Default,
{
    match result {
        Ok(report) => report,
        Err(error) => {
            push_doctor_error(findings, DoctorSeverity::Error, error);
            T::default()
        }
    }
}

fn resolved_doctor_team(
    query: &DoctorQuery,
    config: Option<&config::AtmConfig>,
) -> Option<TeamName> {
    query
        .team_override
        .clone()
        .or_else(|| config::resolve_team(None, config))
}

fn doctor_observability_status(
    observability: &dyn ObservabilityPort,
) -> (crate::observability::AtmObservabilityHealth, DoctorFinding) {
    match observability.health() {
        Ok(health) => {
            let finding = health::observability_finding(&health);
            (health, finding)
        }
        Err(error) => {
            let snapshot = health::unavailable_snapshot(error.to_string());
            let finding = health::observability_finding_from_error(&error);
            (snapshot, finding)
        }
    }
}

fn summarize_doctor_findings(findings: &[DoctorFinding]) -> DoctorSummary {
    let status = health::status_from_findings(findings);
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
    DoctorSummary {
        status,
        message: message.to_string(),
        info_count,
        warning_count,
        error_count,
    }
}

fn load_member_roster(
    runtime: &impl RetainedServiceRuntime,
    home_dir: &Path,
    team: &TeamName,
    config: Option<&config::AtmConfig>,
    findings: &mut Vec<DoctorFinding>,
) -> Option<MembersList> {
    let team_dir = resolve_doctor_team_dir(runtime, home_dir, team, findings)?;
    check_restore_marker(team, &team_dir, findings);
    let (team_config, atm_roster) =
        load_doctor_roster_compare_inputs(runtime, team, &team_dir, findings)?;
    let baseline = config
        .map(|config| config.team_members.as_slice())
        .unwrap_or(&[]);
    check_inbox_directory(team, &team_dir.join("inboxes"), findings);
    record_doctor_roster_drift(team, &team_config, atm_roster, findings);

    Some(MembersList {
        team: team.clone(),
        members: ordered_member_summaries(&team_config.members, baseline),
    })
}

fn resolve_doctor_team_dir(
    runtime: &impl RetainedServiceRuntime,
    home_dir: &Path,
    team: &TeamName,
    findings: &mut Vec<DoctorFinding>,
) -> Option<PathBuf> {
    let team_dir = match runtime.team_dir(home_dir, team) {
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
    Some(team_dir)
}

fn load_doctor_roster_compare_inputs(
    runtime: &impl RetainedServiceRuntime,
    team: &TeamName,
    team_dir: &Path,
    findings: &mut Vec<DoctorFinding>,
) -> Option<(crate::schema::TeamConfig, Option<Vec<RosterMemberRecord>>)> {
    let team_config = match runtime.load_team_config_for_doctor_compare(team_dir) {
        Ok(team_config) => team_config,
        Err(error) => {
            push_doctor_error(findings, DoctorSeverity::Error, error);
            return None;
        }
    };
    let atm_roster = match runtime.load_team_roster(team) {
        Ok(roster) => Some(roster),
        Err(error) => {
            push_doctor_error(findings, DoctorSeverity::Error, error);
            None
        }
    };
    Some((team_config, atm_roster))
}

fn record_doctor_roster_drift(
    team: &TeamName,
    team_config: &crate::schema::TeamConfig,
    atm_roster: Option<Vec<RosterMemberRecord>>,
    findings: &mut Vec<DoctorFinding>,
) {
    let present = team_config
        .members
        .iter()
        .map(|member| member.name.clone())
        .collect::<BTreeSet<_>>();
    let Some(atm_roster) = atm_roster else {
        return;
    };
    let atm_members = atm_roster
        .into_iter()
        .map(|member| member.agent_name)
        .collect::<BTreeSet<_>>();

    for member in atm_members.difference(&present) {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningRosterDrift,
            message: format!(
                "ATM roster member '{member}' is missing from team config.json for '{team}'"
            ),
            remediation: Some(format!(
                "Project the ATM roster back into .claude/teams/{team}/config.json or restore the missing Claude team member before rerunning `atm doctor`."
            )),
        });
    }

    for member in present.difference(&atm_members) {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningRosterDrift,
            message: format!(
                "Claude team member '{member}' is missing from ATM roster truth for '{team}'"
            ),
            remediation: Some(format!(
                "Import the Claude team member into the ATM roster or remove it from .claude/teams/{team}/config.json before rerunning `atm doctor`."
            )),
        });
    }
}

fn push_doctor_error(
    findings: &mut Vec<DoctorFinding>,
    severity: DoctorSeverity,
    error: crate::error::AtmError,
) {
    let remediation = error.primary_recovery().map(str::to_owned);
    findings.push(DoctorFinding {
        severity,
        code: error.code,
        message: error.message,
        remediation,
    });
}

fn check_inbox_directory(team: &TeamName, inboxes_dir: &Path, findings: &mut Vec<DoctorFinding>) {
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

fn check_restore_marker(team: &TeamName, team_dir: &Path, findings: &mut Vec<DoctorFinding>) {
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

    if baseline
        .iter()
        .any(|member| member.as_str() == ROLE_TEAM_LEAD)
        && let Some(team_lead) = members.iter().find(|member| member.name == ROLE_TEAM_LEAD)
    {
        ordered.push(member_summary(team_lead));
        included.insert(team_lead.name.clone());
    }

    for baseline_member in baseline {
        if baseline_member.as_str() == ROLE_TEAM_LEAD {
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
        agent_id: member.agent_id.to_string(),
        agent_type: member.agent_type.to_string(),
        model: member.model.clone(),
        joined_at: member.joined_at,
        tmux_pane_id: member.tmux_pane_id.clone(),
        cwd: member.cwd.display().to_string(),
        extra: member.extra.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::boundary;
    use crate::doctor::{
        DoctorQuery, DoctorReport, DoctorSeverity, DoctorStatus, run_doctor_with_runtime,
    };
    use crate::error::AtmError;
    use crate::error_codes::AtmErrorCode;
    use crate::observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        LogTailSession, ObservabilityPort,
    };
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AgentMember, TeamConfig};
    use crate::service_runtime::LocalServiceRuntime;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, TeamName};

    enum StubHealth {
        Ok(AtmObservabilityHealth),
        Err(AtmError),
    }

    struct StubObservability {
        health: StubHealth,
    }

    impl crate::boundary::sealed::Sealed for StubObservability {}

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

    struct UnusedMailStore;
    struct TestRosterStore {
        members: Vec<boundary::RosterMemberRecord>,
    }

    impl crate::boundary::sealed::Sealed for UnusedMailStore {}
    impl crate::boundary::sealed::Sealed for TestRosterStore {}

    impl boundary::MailStore for UnusedMailStore {
        fn upsert_message(
            &self,
            _record: boundary::MailStoreMessageRecord,
        ) -> Result<(), AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn load_message(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
            _message_key: &boundary::MessageKey,
        ) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn query_mailbox_metadata(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn query_mailbox_metadata_counts(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<boundary::MailStoreMailboxMetadataCounts, AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn upsert_message_state(
            &self,
            _request: boundary::UpsertMailMessageStateRequest,
        ) -> Result<boundary::UpsertMailMessageStateResponse, AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn load_message_state(
            &self,
            _request: boundary::LoadMailMessageStateRequest,
        ) -> Result<boundary::LoadMailMessageStateResponse, AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn record_ingest_replay_state(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
            _source: &boundary::ReplaySource,
            _state: &boundary::MailStoreIngestReplayState,
        ) -> Result<(), AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn load_ingest_replay_state(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
            _source: &boundary::ReplaySource,
        ) -> Result<Option<boundary::MailStoreIngestReplayState>, AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn health_snapshot(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<boundary::MailStoreHealthSnapshot, AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }
    }

    impl boundary::RosterStore for TestRosterStore {
        fn replace_roster(
            &self,
            _team: &TeamName,
            _members: &[boundary::RosterMemberRecord],
            _source: Option<&boundary::ReplaySource>,
        ) -> Result<(), AtmError> {
            unreachable!("doctor tests do not touch the roster store boundary")
        }

        fn load_roster(
            &self,
            _team: &TeamName,
        ) -> Result<Vec<boundary::RosterMemberRecord>, AtmError> {
            Ok(self.members.clone())
        }

        fn query_membership(
            &self,
            _team: &TeamName,
            _member: &AgentName,
        ) -> Result<Option<boundary::RosterMemberRecord>, AtmError> {
            unreachable!("doctor tests do not touch the roster store boundary")
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            unreachable!("doctor tests do not touch the roster store boundary")
        }

        fn health_snapshot(
            &self,
            _team: &TeamName,
        ) -> Result<boundary::RosterStoreHealthSnapshot, AtmError> {
            unreachable!("doctor tests do not touch the roster store boundary")
        }
    }

    fn roster_store(members: &[&str]) -> TestRosterStore {
        TestRosterStore {
            members: members
                .iter()
                .map(|member| boundary::RosterMemberRecord {
                    team_name: TEST_TEAM.parse().expect("team"),
                    agent_name: AgentName::from_validated(*member),
                    member_kind: boundary::RosterMemberKind::Permanent,
                    harness: boundary::RosterHarness::ClaudeCode,
                    agent_type: crate::schema::AgentType::default(),
                    model: crate::types::ModelName::default(),
                    recipient_pane_id: None,
                    metadata_json: serde_json::Map::new(),
                })
                .collect(),
        }
    }

    fn test_runtime_with_roster(
        members: &[&str],
        notification_sink_path: PathBuf,
    ) -> LocalServiceRuntime {
        LocalServiceRuntime::new_with_delivery_boundaries(
            Arc::new(UnusedMailStore),
            Arc::new(roster_store(members)),
            Arc::new(crate::LocalFileNonClaudeOutbound::new()),
            Arc::new(crate::LocalFileNotificationSink::at_path(
                notification_sink_path,
            )),
        )
    }

    fn test_runtime(paths: &TestPaths) -> LocalServiceRuntime {
        test_runtime_with_roster(&[TEST_SENDER], paths.notification_sink_path.clone())
    }

    fn run_doctor(
        paths: &TestPaths,
        query: DoctorQuery,
        observability: &dyn ObservabilityPort,
    ) -> Result<DoctorReport, AtmError> {
        let runtime = test_runtime(paths);
        run_doctor_with_runtime(query, observability, &runtime)
    }

    struct TestPaths {
        _tempdir: tempfile::TempDir,
        home_dir: PathBuf,
        current_dir: PathBuf,
        active_log_path: PathBuf,
        notification_sink_path: PathBuf,
    }

    impl TestPaths {
        fn new() -> Self {
            let tempdir = tempfile::tempdir().expect("tempdir");
            let root = tempdir.path().to_path_buf();
            let home_dir = root.join("atm-home");
            let current_dir = root.join("workspace");
            std::fs::write(root.join(".atm.toml"), "[atm]\n").expect("root sentinel");
            std::fs::create_dir_all(&home_dir).expect("home dir");
            std::fs::create_dir_all(&current_dir).expect("workspace dir");
            Self {
                _tempdir: tempdir,
                home_dir,
                current_dir,
                active_log_path: root.join("atm.log.jsonl"),
                notification_sink_path: root.join("atm-core-doctor-notifications.jsonl"),
            }
        }

        fn team_dir(&self) -> PathBuf {
            self.home_dir.join(".claude").join("teams").join(TEST_TEAM)
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
            team_override: Some(TEST_TEAM.parse().expect("team")),
        }
    }

    #[test]
    fn run_doctor_reports_healthy_observability() {
        let paths = TestPaths::new();
        paths.write_team_layout(&[TEST_SENDER]);
        let report = run_doctor(
            &paths,
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    maintenance: None,
                    diagnostic: None,
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
            &paths,
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
                    maintenance: None,
                    diagnostic: None,
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
        paths.write_team_layout(&[TEST_SENDER]);
        std::fs::write(
            paths.current_dir.join(".atm.toml"),
            format!("[atm]\nidentity = \"{TEST_SENDER}\"\n"),
        )
        .expect("config");
        let report = run_doctor(
            &paths,
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    maintenance: None,
                    diagnostic: None,
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
        paths.write_team_layout(&[TEST_SENDER]);
        let report = run_doctor(
            &paths,
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
                    logging_state: AtmObservabilityHealthState::Degraded,
                    query_state: Some(AtmObservabilityHealthState::Degraded),
                    maintenance: None,
                    diagnostic: None,
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
        paths.write_team_layout(&[TEST_SENDER]);
        let report = run_doctor(
            &paths,
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: None,
                    logging_state: AtmObservabilityHealthState::Unavailable,
                    query_state: Some(AtmObservabilityHealthState::Unavailable),
                    maintenance: None,
                    diagnostic: None,
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
        paths.write_team_layout(&[TEST_SENDER]);
        let report = run_doctor(
            &paths,
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
            &paths,
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    maintenance: None,
                    diagnostic: None,
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
            &paths,
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    maintenance: None,
                    diagnostic: None,
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
        paths.write_raw_team_config(&format!(r#"{{"members":[{{"name":"{TEST_SENDER}"}}]}}"#));
        let report = run_doctor(
            &paths,
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    maintenance: None,
                    diagnostic: None,
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
    fn run_doctor_reports_atm_roster_and_claude_roster_drift_as_warning() {
        let paths = TestPaths::new();
        paths.write_team_layout(&[TEST_SENDER]);
        let runtime = test_runtime_with_roster(
            &[TEST_SENDER, ROLE_TEAM_LEAD],
            paths.notification_sink_path.clone(),
        );
        let report = run_doctor_with_runtime(
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    maintenance: None,
                    diagnostic: None,
                    detail: None,
                }),
            },
            &runtime,
        )
        .expect("doctor report");

        assert_eq!(report.summary.status, DoctorStatus::Warning);
        assert!(
            report.findings.iter().any(|finding| {
                finding.code == AtmErrorCode::WarningRosterDrift
                    && finding.message.contains(&format!(
                        "ATM roster member '{}' is missing from team config.json",
                        ROLE_TEAM_LEAD
                    ))
            }),
            "{report:#?}"
        );
    }

    #[test]
    fn run_doctor_reports_stale_mailbox_lock_as_warning() {
        let paths = TestPaths::new();
        paths.write_team_layout(&[TEST_SENDER]);
        let stale_lock = paths
            .team_dir()
            .join("inboxes")
            .join(format!("{TEST_SENDER}.json.lock"));
        std::fs::write(&stale_lock, u32::MAX.to_string()).expect("stale lock");
        let report = run_doctor(
            &paths,
            query(&paths),
            &StubObservability {
                health: StubHealth::Ok(AtmObservabilityHealth {
                    active_log_path: Some(paths.active_log_path.clone()),
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    maintenance: None,
                    diagnostic: None,
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
