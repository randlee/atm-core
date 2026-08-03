pub mod health;
pub mod report;

#[cfg(test)]
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::boundary::{ConfigDoctor, MailStoreDoctor, RosterStoreDoctor};
use crate::config;
use crate::error_codes::AtmErrorCode;
use crate::observability::ObservabilityPort;
#[cfg(test)]
use crate::roles::ROLE_TEAM_LEAD;
#[cfg(test)]
use crate::schema::AgentMember;
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::default_runtime;
use crate::team_admin::{MembersList, ordered_roster_member_summaries};
use crate::types::{AgentName, TeamName};
use atm_storage::PeerConfigStore;
use std::sync::Arc;

pub use report::{
    BootstrapAutoStartOutcome, BootstrapConnectOutcome, BootstrapLaunchGateOutcome,
    BootstrapTraceReport, DaemonRuntimeDoctorReport, DoctorEnvironmentVisibility,
    DoctorExecutionContext, DoctorFinding, DoctorReport, DoctorSeverity, DoctorStatus,
    DoctorSummary, PeerAuthorityDoctorReport, PeerConfigDoctorReport, PeerDrainState,
    PeerLinkQuality, PeerLinkStatus, PostSendDoctorReport, PostSendHookRuleIndex,
    PostSendHookRuleReport, RecipientDeliveryPath, RecipientDeliveryPathReport,
};

/// Inputs for a doctor run, including the caller's resolved identity.
///
/// When the doctor request is serviced over IPC by the long-lived daemon
/// singleton, the daemon process cannot observe the requesting shell's
/// `ATM_TEAM`/`ATM_IDENTITY`; its own process environment is frozen at launch
/// time. The `caller_team` and `caller_identity` fields carry the invoking
/// CLI process's resolved values across the IPC boundary so the resulting
/// report's `client_context` reflects the real caller rather than whatever
/// environment happens to be visible where the report is evaluated.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DoctorQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub team_override: Option<TeamName>,
    /// Caller's `ATM_TEAM`, captured in the invoking CLI process.
    #[serde(default)]
    pub caller_team: Option<TeamName>,
    /// Caller's `ATM_IDENTITY`, captured in the invoking CLI process.
    #[serde(default)]
    pub caller_identity: Option<AgentName>,
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
    let doctor_context = doctor_run_context(&query, config.as_ref());
    let (observability_health, finding) = doctor_observability_status(observability);
    let mut findings = Vec::new();
    push_obsolete_identity_warning(config.as_ref(), &mut findings);
    let member_roster = doctor_context.resolved_team.as_ref().and_then(|team| {
        load_member_roster(
            runtime,
            team,
            doctor_context.environment.atm_identity.as_ref(),
            Some(query.current_dir.as_path()),
            &mut findings,
        )
    });
    findings.push(finding);
    Ok(build_doctor_report(
        findings,
        doctor_context.environment,
        member_roster,
        PostSendDoctorReport::default(),
        crate::boundary::ConfigDoctorReport::default(),
        crate::boundary::MailStoreDoctorReport::default(),
        crate::boundary::RosterStoreDoctorReport::default(),
        None,
        Vec::new(),
        observability_health,
        None,
        None,
    ))
}

pub fn run_doctor_with_runtime_ports(
    query: DoctorQuery,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
    runtime_doctors: &RuntimeDoctorPorts,
    daemon_runtime: Option<report::DaemonRuntimeDoctorReport>,
) -> Result<DoctorReport, crate::error::AtmError> {
    let config = runtime.load_config(&query.current_dir)?;
    let doctor_context = doctor_run_context(&query, config.as_ref());
    let (observability_health, observability_finding) = doctor_observability_status(observability);
    let mut general_findings = Vec::new();
    let mut drift_findings = Vec::new();
    let mut reports = inspect_runtime_doctor_sections(runtime_doctors, &mut general_findings);
    push_obsolete_identity_finding(config.as_ref(), &mut reports.config);
    let member_roster = doctor_context.resolved_team.as_ref().and_then(|team| {
        load_member_roster(
            runtime,
            team,
            doctor_context.environment.atm_identity.as_ref(),
            Some(query.current_dir.as_path()),
            &mut drift_findings,
        )
    });
    let findings = collect_doctor_findings(
        &reports,
        &drift_findings,
        &general_findings,
        observability_finding,
        daemon_runtime.as_ref(),
    );
    let post_send = post_send_doctor_report(
        config.as_ref(),
        member_roster.as_ref(),
        runtime,
        doctor_context.resolved_team.as_ref(),
    );

    Ok(build_doctor_report(
        findings,
        doctor_context.environment,
        member_roster,
        post_send,
        reports.config,
        reports.mail_store,
        reports.roster_store,
        daemon_runtime,
        drift_findings,
        observability_health,
        None,
        None,
    ))
}

/// Project safe peer-control-plane state into doctor output. A storage or
/// validation failure is report data, never a reason for `atm doctor` to abort.
pub fn peer_config_doctor_report(
    store: &(dyn PeerConfigStore + Send + Sync),
) -> (PeerConfigDoctorReport, Vec<DoctorFinding>) {
    match peer_config_doctor_report_inner(store) {
        Ok(report) => (report, Vec::new()),
        Err(error) => {
            let finding = DoctorFinding {
                severity: DoctorSeverity::Error,
                code: error.code(),
                message: error.message().to_string(),
                remediation: Some(
                    "Repair the peer HTTPS configuration or bind conflict, then rerun `atm doctor`."
                        .to_string(),
                ),
            };
            (
                PeerConfigDoctorReport {
                    validation_failure: Some(finding.clone()),
                    ..PeerConfigDoctorReport::default()
                },
                vec![finding],
            )
        }
    }
}

fn peer_config_doctor_report_inner(
    store: &(dyn PeerConfigStore + Send + Sync),
) -> Result<PeerConfigDoctorReport, crate::error::AtmError> {
    let interfaces = store.list_interfaces()?;
    let peers = store.list_trusted_peers()?;
    let certificate = store.local_certificate()?;
    Ok(PeerConfigDoctorReport {
        configured_interface_count: interfaces.len(),
        enabled_interface_count: interfaces
            .iter()
            .filter(|interface| interface.enabled)
            .count(),
        certificate_fingerprint: certificate.map(|certificate| certificate.fingerprint.to_string()),
        trusted_peer_count: peers.len(),
        enabled_trusted_peer_count: peers.iter().filter(|peer| peer.enabled).count(),
        trusted_peers: peers
            .iter()
            .map(|peer| PeerAuthorityDoctorReport {
                host: peer.host.to_string(),
                https_port: peer.https_port.get(),
                enabled: peer.enabled,
            })
            .collect(),
        validation_failure: None,
    })
}

struct DoctorRunContext {
    resolved_team: Option<TeamName>,
    environment: DoctorEnvironmentVisibility,
}

fn doctor_run_context(
    query: &DoctorQuery,
    config: Option<&crate::config::AtmConfig>,
) -> DoctorRunContext {
    DoctorRunContext {
        resolved_team: resolved_doctor_team(query, config),
        environment: health::environment_visibility(
            query.home_dir.clone(),
            query.team_override.clone(),
            query.caller_team.clone(),
            query.caller_identity.clone(),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_doctor_report(
    findings: Vec<DoctorFinding>,
    environment: DoctorEnvironmentVisibility,
    member_roster: Option<MembersList>,
    post_send: PostSendDoctorReport,
    config: crate::boundary::ConfigDoctorReport,
    mail_store: crate::boundary::MailStoreDoctorReport,
    roster_store: crate::boundary::RosterStoreDoctorReport,
    daemon_runtime: Option<report::DaemonRuntimeDoctorReport>,
    drift_findings: Vec<DoctorFinding>,
    observability_health: crate::observability::AtmObservabilityHealth,
    runtime_status: Option<crate::protocol::RuntimeStatusSnapshot>,
    bootstrap_trace: Option<BootstrapTraceReport>,
) -> DoctorReport {
    let summary = summarize_doctor_findings(&findings);
    let recommendations = collect_recommendations(&findings);
    DoctorReport {
        summary,
        findings,
        recommendations,
        environment: environment.clone(),
        client_context: doctor_client_context(&environment),
        daemon_context: None,
        member_roster,
        observability: observability_health,
        post_send,
        config,
        mail_store,
        roster_store,
        daemon_runtime,
        drift_findings,
        runtime_status,
        bootstrap_trace,
    }
}

fn doctor_client_context(environment: &DoctorEnvironmentVisibility) -> DoctorExecutionContext {
    DoctorExecutionContext {
        // A `--team` override reflects the team the caller explicitly asked
        // the doctor to inspect, so it takes precedence over the ambient
        // `ATM_TEAM` in the reported client context.
        team: environment
            .team_override
            .clone()
            .or_else(|| environment.atm_team.clone()),
        identity: environment.atm_identity.clone(),
        version: Some(crate::protocol::ReleaseVersion::current()),
        cli_schema_version: Some(crate::protocol::CLI_SCHEMA_VERSION),
        http_api_version: Some(crate::protocol::HttpApiVersion::current()),
    }
}

fn push_obsolete_identity_warning(
    config: Option<&crate::config::AtmConfig>,
    findings: &mut Vec<DoctorFinding>,
) {
    if config.is_some_and(|config| config.obsolete_identity.is_some()) {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningIdentityDrift,
            message: "obsolete config identity is still present in .atm.toml (`[atm].identity` or legacy top-level `identity`); ATM no longer uses config identity as a runtime fallback.".to_string(),
            remediation: Some(
                "Remove `[atm].identity` or the legacy top-level `identity` key from `.atm.toml` and set `ATM_IDENTITY` in the active agent environment instead."
                    .to_string(),
            ),
        });
    }
}

fn post_send_doctor_report(
    config: Option<&crate::config::AtmConfig>,
    member_roster: Option<&MembersList>,
    runtime: &LocalServiceRuntime,
    team: Option<&TeamName>,
) -> PostSendDoctorReport {
    let Some(config) = config else {
        return PostSendDoctorReport::default();
    };
    let external_rules = config
        .post_send_hooks
        .iter()
        .filter_map(|rule| {
            let (program, argv) = rule.command.split_first()?;
            Some(PostSendHookRuleReport {
                recipient_matcher: rule.recipient.to_string(),
                executable: std::path::PathBuf::from(program),
                argv: argv.to_vec(),
                config_root: config.config_root.clone(),
            })
        })
        .collect::<Vec<_>>();
    // A disabled built-in delivery template is a team-level delivery policy.
    // Report it before external matching so doctor describes the path that will
    // actually be emitted, without exposing rendered message content.
    let built_in_delivery_disabled = team
        .and_then(|team| {
            runtime
                .load_nudge_template_override(
                    team,
                    crate::boundary::BuiltInNudgeTemplateKind::Delivery,
                )
                .ok()
                .flatten()
        })
        .is_some_and(|row| {
            matches!(
                row.mode,
                crate::boundary::TeamNudgeTemplateOverrideMode::Disabled
            )
        });
    let recipient_paths = member_roster
        .into_iter()
        .flat_map(|roster| roster.members.iter())
        .map(|member| {
            let path = if built_in_delivery_disabled {
                RecipientDeliveryPath::Disabled
            } else {
                config
                    .post_send_hooks
                    .iter()
                    .position(|rule| rule.recipient.matches(&member.name))
                    .and_then(|index| u32::try_from(index).ok())
                    .map(|rule| RecipientDeliveryPath::ExternalOverride {
                        rule: PostSendHookRuleIndex(rule),
                    })
                    .unwrap_or(RecipientDeliveryPath::BuiltIn)
            };
            RecipientDeliveryPathReport {
                recipient: member.name.clone(),
                path,
            }
        })
        .collect();
    PostSendDoctorReport {
        config_root: config.config_root.clone(),
        external_rules,
        recipient_paths,
    }
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
    if config.is_some_and(|config| config.obsolete_identity.is_some()) {
        config_report.findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::WarningIdentityDrift,
            message: "obsolete config identity is still present in .atm.toml (`[atm].identity` or legacy top-level `identity`); ATM no longer uses config identity as a runtime fallback.".to_string(),
            remediation: Some(
                "Remove `[atm].identity` or the legacy top-level `identity` key from `.atm.toml` and set `ATM_IDENTITY` in the active agent environment instead."
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
    team: &TeamName,
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
    findings: &mut Vec<DoctorFinding>,
) -> Option<MembersList> {
    if let Err(error) = crate::address::validate_path_segment(team.as_str(), "team") {
        push_doctor_error(findings, DoctorSeverity::Error, error);
        return None;
    }
    let members = match runtime.load_team_roster(team) {
        Ok(roster) => ordered_roster_member_summaries(&roster, caller_identity, live_cwd),
        Err(error) => {
            push_doctor_error(findings, DoctorSeverity::Error, error);
            return None;
        }
    };

    Some(MembersList {
        team: team.clone(),
        members,
    })
}

fn push_doctor_error(
    findings: &mut Vec<DoctorFinding>,
    severity: DoctorSeverity,
    error: crate::error::AtmError,
) {
    let remediation = Some(error.message().to_owned());
    findings.push(DoctorFinding {
        severity,
        code: error.code(),
        message: error.into_message(),
        remediation,
    });
}

#[cfg(test)]
fn ordered_member_summaries(
    members: &[AgentMember],
    baseline: &[TeamName],
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
) -> Vec<crate::team_admin::MemberSummary> {
    let mut ordered = Vec::new();
    let mut included = BTreeSet::new();

    if baseline
        .iter()
        .any(|member| member.as_str() == ROLE_TEAM_LEAD)
        && let Some(team_lead) = members.iter().find(|member| member.name == ROLE_TEAM_LEAD)
    {
        ordered.push(member_summary(team_lead, caller_identity, live_cwd));
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
            ordered.push(member_summary(member, caller_identity, live_cwd));
            included.insert(member.name.clone());
        }
    }

    for member in members {
        if included.insert(member.name.clone()) {
            ordered.push(member_summary(member, caller_identity, live_cwd));
        }
    }

    ordered
}

#[cfg(test)]
fn member_summary(
    member: &AgentMember,
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
) -> crate::team_admin::MemberSummary {
    crate::team_admin::MemberSummary {
        name: AgentName::from_validated(member.name.clone()),
        agent_id: member.agent_id.to_string(),
        agent_type: member.agent_type.to_string(),
        harness: crate::boundary::RosterHarness::ClaudeCode,
        model: member.model.clone(),
        joined_at: member.joined_at,
        tmux_pane_id: member.tmux_pane_id.clone(),
        home_dir: member.home_dir.clone(),
        live_cwd: match (caller_identity, live_cwd) {
            (Some(identity), Some(path)) if member.name == identity.as_str() => {
                Some(path.display().to_string())
            }
            _ => None,
        },
        extra: member.extra.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::{ordered_member_summaries, peer_config_doctor_report};
    use crate::config::AtmConfig;
    use crate::config::types::{HookRecipient, PostSendHookRule};
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
    use crate::schema::{AgentMember, HOME_DIR_METADATA_KEY, TeamConfig};
    use crate::service_runtime::LocalServiceRuntime;
    use crate::team_admin::MembersList;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, TeamName};
    use atm_storage::{
        CertificateFingerprint, HostName, HttpsInterface, LocalCertificate, PeerConfigStore,
        PrivateKeyRef, TrustedPeer,
    };

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
                StubHealth::Err(error) => Err(error.clone()),
            }
        }
    }

    struct UnusedMailStore;
    struct TestRosterStore {
        members: Vec<atm_storage::RosterMember>,
    }
    struct NoopNudgeTemplateOverrideStore;

    impl atm_storage::contract::sealed::Sealed for UnusedMailStore {}
    impl atm_storage::contract::sealed::Sealed for TestRosterStore {}

    struct StubPeerConfigStore {
        failure: Option<AtmError>,
    }

    impl atm_storage::contract::sealed::Sealed for StubPeerConfigStore {}

    impl StubPeerConfigStore {
        fn healthy() -> Self {
            Self { failure: None }
        }

        fn failing(error: AtmError) -> Self {
            Self {
                failure: Some(error),
            }
        }

        fn result<T>(&self, value: T) -> Result<T, AtmError> {
            self.failure.clone().map_or(Ok(value), Err)
        }
    }

    impl PeerConfigStore for StubPeerConfigStore {
        fn list_interfaces(&self) -> Result<Vec<HttpsInterface>, AtmError> {
            self.result(vec![HttpsInterface {
                bind_addr: "127.0.0.1:43101".parse().expect("socket address"),
                advertise_host: "localhost".parse().expect("host name"),
                enabled: true,
            }])
        }

        fn save_interface(&self, _interface: &HttpsInterface) -> Result<(), AtmError> {
            unreachable!("doctor test never mutates peer configuration")
        }

        fn remove_interface(&self, _bind_addr: std::net::SocketAddr) -> Result<bool, AtmError> {
            unreachable!("doctor test never mutates peer configuration")
        }

        fn local_certificate(&self) -> Result<Option<LocalCertificate>, AtmError> {
            self.result(Some(LocalCertificate {
                fingerprint: "sha256:local"
                    .parse::<CertificateFingerprint>()
                    .expect("fingerprint"),
                private_key_ref: "keychain:secret"
                    .parse::<PrivateKeyRef>()
                    .expect("key reference"),
            }))
        }

        fn save_local_certificate(&self, _certificate: &LocalCertificate) -> Result<(), AtmError> {
            unreachable!("doctor test never mutates peer configuration")
        }

        fn list_trusted_peers(&self) -> Result<Vec<TrustedPeer>, AtmError> {
            self.result(vec![TrustedPeer {
                host: "peer.example".parse::<HostName>().expect("host name"),
                fingerprint: "sha256:peer"
                    .parse::<CertificateFingerprint>()
                    .expect("fingerprint"),
                enabled: true,
                https_port: std::num::NonZeroU16::new(43101).expect("non-zero port"),
            }])
        }

        fn trusted_peer(&self, _host: &HostName) -> Result<Option<TrustedPeer>, AtmError> {
            unreachable!("doctor test never reads an individual peer")
        }

        fn save_trusted_peer(&self, _peer: &TrustedPeer) -> Result<(), AtmError> {
            unreachable!("doctor test never mutates peer configuration")
        }

        fn remove_trusted_peer(&self, _host: &HostName) -> Result<bool, AtmError> {
            unreachable!("doctor test never mutates peer configuration")
        }
    }

    #[allow(
        deprecated,
        reason = "doctor tests intentionally exercise the transitional shared storage traits"
    )]
    impl atm_storage::MessageStore for UnusedMailStore {
        fn save_message(&self, _message: &atm_storage::Message) -> Result<(), AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn save_messages_atomically(
            &self,
            _messages: &[atm_storage::Message],
        ) -> Result<(), AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn load_message(
            &self,
            _message_key: &atm_storage::MessageKey,
        ) -> Result<Option<atm_storage::Message>, AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn list_messages(
            &self,
            _query: &atm_storage::MessageQuery,
        ) -> Result<Vec<atm_storage::Message>, AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }

        fn delete_message(&self, _key: &atm_storage::MessageKey) -> Result<(), AtmError> {
            unreachable!("doctor tests do not touch the mail store boundary")
        }
    }

    #[allow(
        deprecated,
        reason = "doctor tests intentionally exercise the transitional shared storage traits"
    )]
    impl atm_storage::RosterStore for TestRosterStore {
        fn load_roster(&self, team: &TeamName) -> Result<atm_storage::RosterSnapshot, AtmError> {
            Ok(atm_storage::RosterSnapshot {
                team_name: team.clone(),
                members: self.members.clone(),
                refreshed_at: None,
            })
        }

        fn save_roster(&self, _roster: &atm_storage::RosterSnapshot) -> Result<(), AtmError> {
            unreachable!("doctor tests do not touch the roster store boundary")
        }

        fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
            unreachable!("doctor tests do not touch the roster store boundary")
        }
    }

    impl atm_storage::contract::sealed::Sealed for NoopNudgeTemplateOverrideStore {}

    impl crate::boundary::NudgeTemplateOverrideStore for NoopNudgeTemplateOverrideStore {
        fn load_template_override(
            &self,
            _team: &TeamName,
            _kind: crate::boundary::BuiltInNudgeTemplateKind,
        ) -> Result<Option<crate::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
            Ok(None)
        }

        fn save_template_override(
            &self,
            _team: &TeamName,
            _kind: crate::boundary::BuiltInNudgeTemplateKind,
            _template_body: &str,
        ) -> Result<crate::boundary::TeamNudgeTemplateOverrideRow, AtmError> {
            unreachable!("doctor tests do not touch the override-store boundary")
        }

        fn disable_template_override(
            &self,
            _team: &TeamName,
            _kind: crate::boundary::BuiltInNudgeTemplateKind,
        ) -> Result<crate::boundary::TeamNudgeTemplateOverrideRow, AtmError> {
            unreachable!("doctor tests do not touch the override-store boundary")
        }

        fn clear_template_override(
            &self,
            _team: &TeamName,
            _kind: crate::boundary::BuiltInNudgeTemplateKind,
        ) -> Result<bool, AtmError> {
            unreachable!("doctor tests do not touch the override-store boundary")
        }
    }

    fn roster_store(members: &[&str]) -> TestRosterStore {
        TestRosterStore {
            members: members
                .iter()
                .map(|member| atm_storage::RosterMember {
                    team_name: TEST_TEAM.parse().expect("team"),
                    agent_name: AgentName::from_validated(*member),
                    member_kind: atm_storage::RosterMemberKind::Permanent,
                    harness: atm_storage::RosterHarness::ClaudeCode,
                    agent_type: atm_storage::contract::AgentType::default(),
                    model: atm_storage::ModelName::default(),
                    recipient_pane_id: None,
                    metadata_json: serde_json::Map::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn post_send_report_projects_external_override_without_message_content() {
        let config = AtmConfig {
            config_root: PathBuf::from("/workspace"),
            post_send_hooks: vec![PostSendHookRule {
                recipient: HookRecipient::Named(TEST_SENDER.parse().expect("recipient")),
                command: vec!["hooks/nudge".to_string(), "--quiet".to_string()],
            }],
            ..AtmConfig::default()
        };
        let roster = MembersList {
            team: TEST_TEAM.parse().expect("team"),
            members: vec![crate::team_admin::MemberSummary {
                name: TEST_SENDER.parse().expect("member"),
                agent_id: TEST_SENDER.to_string(),
                agent_type: "general".to_string(),
                harness: crate::boundary::RosterHarness::ClaudeCode,
                model: Default::default(),
                joined_at: None,
                tmux_pane_id: None,
                home_dir: PathBuf::from("/workspace").into(),
                live_cwd: None,
                extra: serde_json::Map::new(),
            }],
        };

        let runtime = test_runtime_with_roster(&[TEST_SENDER]);
        let report = super::post_send_doctor_report(
            Some(&config),
            Some(&roster),
            &runtime,
            Some(&roster.team),
        );

        assert_eq!(report.external_rules.len(), 1);
        assert_eq!(
            report.external_rules[0].executable,
            PathBuf::from("hooks/nudge")
        );
        assert_eq!(report.external_rules[0].argv, ["--quiet"]);
        assert!(matches!(
            report.recipient_paths.as_slice(),
            [crate::doctor::RecipientDeliveryPathReport {
                path: crate::doctor::RecipientDeliveryPath::ExternalOverride {
                    rule: crate::doctor::PostSendHookRuleIndex(0),
                },
                ..
            }]
        ));
        assert!(
            !serde_json::to_string(&report)
                .expect("serialize report")
                .contains("message-body-must-never-appear")
        );
    }

    fn test_runtime_with_roster(members: &[&str]) -> LocalServiceRuntime {
        LocalServiceRuntime::new_with_delivery_boundaries(
            Arc::new(UnusedMailStore),
            Arc::new(roster_store(members)),
            Arc::new(NoopNudgeTemplateOverrideStore),
            Arc::new(crate::LocalFileNonClaudeOutbound::new()),
        )
    }

    fn test_runtime(paths: &TestPaths) -> LocalServiceRuntime {
        let _ = paths;
        test_runtime_with_roster(&[TEST_SENDER])
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
            ..DoctorQuery::default()
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
                ..DoctorQuery::default()
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
                .contains("obsolete config identity")
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
    fn run_doctor_ignores_missing_team_directory_without_roster_truth_error() {
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

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.code != AtmErrorCode::TeamNotFound),
            "{report:#?}"
        );
    }

    #[test]
    fn run_doctor_ignores_team_config_parse_failure() {
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

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.code != AtmErrorCode::ConfigTeamParseFailed),
            "{report:#?}"
        );
    }

    #[test]
    fn run_doctor_ignores_missing_inboxes_directory() {
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

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.code != AtmErrorCode::MailboxWriteFailed),
            "{report:#?}"
        );
    }

    #[test]
    fn run_doctor_uses_atm_roster_without_claude_roster_drift_checks() {
        let paths = TestPaths::new();
        paths.write_team_layout(&[TEST_SENDER]);
        let runtime = test_runtime_with_roster(&[TEST_SENDER, ROLE_TEAM_LEAD]);
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

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.code != AtmErrorCode::WarningRosterDrift),
            "{report:#?}"
        );
        let member_roster = report.member_roster.expect("member roster");
        assert_eq!(member_roster.members.len(), 2);
    }

    #[test]
    fn run_doctor_reports_member_metadata_from_roster_truth() {
        let paths = TestPaths::new();
        paths.write_team_layout(&[TEST_SENDER]);
        paths.write_raw_team_config(&format!(
            r#"{{"members":[{{"name":"{TEST_SENDER}","home_dir":"/repo/config"}}]}}"#
        ));
        let mut roster_member = atm_storage::RosterMember {
            team_name: TEST_TEAM.parse().expect("team"),
            agent_name: AgentName::from_validated(TEST_SENDER),
            member_kind: atm_storage::RosterMemberKind::Permanent,
            harness: atm_storage::RosterHarness::ClaudeCode,
            agent_type: atm_storage::contract::AgentType::default(),
            model: atm_storage::ModelName::default(),
            recipient_pane_id: Some(crate::types::PaneId::from_cli("%9").expect("pane")),
            metadata_json: serde_json::Map::new(),
        };
        roster_member.metadata_json.insert(
            HOME_DIR_METADATA_KEY.to_string(),
            serde_json::json!("/repo/roster"),
        );
        let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
            Arc::new(UnusedMailStore),
            Arc::new(TestRosterStore {
                members: vec![roster_member],
            }),
            Arc::new(NoopNudgeTemplateOverrideStore),
            Arc::new(crate::LocalFileNonClaudeOutbound::new()),
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

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| !finding.message.contains("pane drift")),
            "{report:#?}"
        );
        assert!(
            report
                .findings
                .iter()
                .all(|finding| !finding.message.contains("home-dir drift")),
            "{report:#?}"
        );
        let member_roster = report.member_roster.expect("member roster");
        assert_eq!(member_roster.members[0].tmux_pane_id.as_deref(), Some("%9"));
        assert_eq!(
            member_roster.members[0].home_dir.as_path(),
            Path::new("/repo/roster")
        );
    }

    #[test]
    fn ordered_member_summaries_overlay_live_cwd_for_calling_member_only() {
        let members = vec![
            AgentMember::with_name(AgentName::from_validated(ROLE_TEAM_LEAD)),
            AgentMember::with_name(AgentName::from_validated(TEST_SENDER)),
        ];
        let caller_identity = AgentName::from_validated(TEST_SENDER);

        let summaries = ordered_member_summaries(
            &members,
            &[],
            Some(&caller_identity),
            Some(Path::new("/repo/live")),
        );

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].name.as_str(), ROLE_TEAM_LEAD);
        assert_eq!(summaries[0].live_cwd, None);
        assert_eq!(summaries[1].name.as_str(), TEST_SENDER);
        assert_eq!(summaries[1].live_cwd.as_deref(), Some("/repo/live"));
    }

    #[test]
    fn run_doctor_ignores_stale_mailbox_lock_scan() {
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

        assert_eq!(report.summary.status, DoctorStatus::Healthy);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.code != AtmErrorCode::WarningStaleMailboxLock),
            "{report:#?}"
        );
    }

    fn healthy_observability(paths: &TestPaths) -> StubObservability {
        StubObservability {
            health: StubHealth::Ok(AtmObservabilityHealth {
                active_log_path: Some(paths.active_log_path.clone()),
                logging_state: AtmObservabilityHealthState::Healthy,
                query_state: Some(AtmObservabilityHealthState::Healthy),
                maintenance: None,
                diagnostic: None,
                detail: None,
            }),
        }
    }

    #[test]
    #[serial_test::serial(env)]
    fn client_context_reflects_caller_not_ambient_environment() {
        let paths = TestPaths::new();
        paths.write_team_layout(&[TEST_SENDER]);
        // Ambient process env stands in for the long-lived daemon's frozen
        // launch-time identity; the caller's threaded values must win.
        let _env = crate::test_support::EnvGuard::set_many([
            ("ATM_TEAM", Some("daemon-launch-team")),
            ("ATM_IDENTITY", Some("daemon-launch-identity")),
        ]);
        let query = DoctorQuery {
            home_dir: paths.home_dir.clone(),
            current_dir: paths.current_dir.clone(),
            team_override: None,
            caller_team: Some(TEST_TEAM.parse().expect("team")),
            caller_identity: Some(TEST_SENDER.parse().expect("identity")),
        };

        let report =
            run_doctor(&paths, query, &healthy_observability(&paths)).expect("doctor report");

        assert_eq!(
            report.client_context.team.as_ref().map(TeamName::as_str),
            Some(TEST_TEAM)
        );
        assert_eq!(
            report
                .client_context
                .identity
                .as_ref()
                .map(AgentName::as_str),
            Some(TEST_SENDER)
        );
        assert_eq!(
            report.environment.atm_team.as_ref().map(TeamName::as_str),
            Some(TEST_TEAM)
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn team_override_is_reflected_in_client_context() {
        let paths = TestPaths::new();
        paths.write_team_layout(&[TEST_SENDER]);
        let _env =
            crate::test_support::EnvGuard::set_many([("ATM_TEAM", None), ("ATM_IDENTITY", None)]);
        let override_team = format!("{TEST_TEAM}-override");
        let query = DoctorQuery {
            home_dir: paths.home_dir.clone(),
            current_dir: paths.current_dir.clone(),
            team_override: Some(override_team.parse().expect("team override")),
            caller_team: Some(TEST_TEAM.parse().expect("team")),
            caller_identity: Some(TEST_SENDER.parse().expect("identity")),
        };

        let report =
            run_doctor(&paths, query, &healthy_observability(&paths)).expect("doctor report");

        assert_eq!(
            report.client_context.team.as_ref().map(TeamName::as_str),
            Some(override_team.as_str())
        );
    }

    #[test]
    fn peer_config_doctor_projection_redacts_private_key_reference_from_store() {
        let (report, findings) = peer_config_doctor_report(&StubPeerConfigStore::healthy());

        assert!(findings.is_empty());
        assert_eq!(report.configured_interface_count, 1);
        assert_eq!(report.enabled_interface_count, 1);
        assert_eq!(report.trusted_peer_count, 1);
        assert_eq!(report.enabled_trusted_peer_count, 1);
        assert_eq!(
            report.certificate_fingerprint.as_deref(),
            Some("sha256:local")
        );
        assert!(report.validation_failure.is_none());

        let serialized = serde_json::to_string(&report).expect("serialize doctor projection");
        assert!(!serialized.contains("keychain:secret"));
        assert!(!serialized.contains("private_key_ref"));
    }

    #[test]
    fn peer_config_doctor_projects_configuration_failure_without_aborting() {
        let (report, findings) = peer_config_doctor_report(&StubPeerConfigStore::failing(
            AtmError::peer_config_validation("missing certificate reference"),
        ));

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, AtmErrorCode::PeerConfigValidationFailed);
        assert_eq!(
            report
                .validation_failure
                .as_ref()
                .map(|finding| finding.code),
            Some(AtmErrorCode::PeerConfigValidationFailed)
        );
    }
}
