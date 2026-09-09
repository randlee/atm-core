//! Concrete endpoint diagnostics over the private Herdr transport seam.

use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use atm_core::doctor::{
    DoctorFinding, DoctorSeverity, HerdrBinaryProvenance, HerdrBinaryResolution, HerdrDoctorState,
    HerdrEndpointObservation, HerdrEndpointProvenance, HerdrMemberPresence, HerdrPresenceOutcome,
    HerdrRosterMember, HerdrVersion,
};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::{HerdrSession, RequestDeadline};

use crate::transport::{
    HerdrIo, HerdrOp, get_from_envelope, list_from_envelope, server_status_from_envelope,
};
use crate::{HERDR_MINIMUM_VERSION, HerdrClientConfig, HerdrError, HerdrListOutcome};

/// Production endpoint probe. Construction selects the configured client
/// transport but does not execute a Herdr command.
#[derive(Clone, Debug)]
pub struct HerdrDoctorProbe {
    config: HerdrClientConfig,
    io: HerdrIo,
}

impl HerdrDoctorProbe {
    #[must_use]
    pub fn new(config: HerdrClientConfig) -> Self {
        Self {
            io: HerdrIo::from_config(&config),
            config,
        }
    }

    /// Observes one endpoint once, followed by at most one routed-member
    /// lookup per supplied roster member. Probe requests deliberately bypass
    /// the send-path breaker: diagnostics must report the endpoint itself.
    pub async fn observe(
        &self,
        session: Option<&HerdrSession>,
        members: &[HerdrRosterMember],
        deadline: RequestDeadline,
    ) -> HerdrEndpointObservation {
        let started = Instant::now();
        let mut observation = HerdrEndpointObservation {
            session: session.cloned(),
            provenance: endpoint_provenance(&self.config, session),
            transport: self.config.transport().clone(),
            endpoint: self.io.endpoint_display(session),
            binary: self.binary_resolution(),
            state: HerdrDoctorState::NotConfigured,
            live_handoff: None,
            members: Vec::new(),
            findings: Vec::new(),
        };

        let status = self
            .io
            .call(HerdrOp::StatusServer, session, deadline)
            .await
            .and_then(server_status_from_envelope);
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                observation.findings = stale_session_findings(session, members, &error);
                observation.state = self.state_for_error(error, started.elapsed(), session);
                return observation;
            }
        };

        observation.live_handoff = Some(status.live_handoff);
        observation.state = state_for_server(status.version, status.protocol);
        observation.members = self.observe_members(session, members, deadline).await;
        observation
    }

    fn binary_resolution(&self) -> Option<HerdrBinaryResolution> {
        self.config.binary_path().map(|path| HerdrBinaryResolution {
            path: path.to_path_buf(),
            provenance: HerdrBinaryProvenance::Configured,
        })
    }

    async fn observe_members(
        &self,
        session: Option<&HerdrSession>,
        members: &[HerdrRosterMember],
        deadline: RequestDeadline,
    ) -> Vec<HerdrMemberPresence> {
        let mut observations = Vec::with_capacity(members.len());
        let mut listed_agents = None;
        for member in members {
            let member_deadline = remaining_member_deadline(deadline);
            let outcome = match self
                .io
                .call(
                    HerdrOp::Get {
                        agent: &member.herdr_agent,
                    },
                    session,
                    member_deadline,
                )
                .await
                .and_then(get_from_envelope)
                .map(|_| HerdrPresenceOutcome::Visible)
            {
                Ok(outcome) => outcome,
                Err(error @ (HerdrError::AgentNotFound | HerdrError::AgentTargetAmbiguous)) => {
                    if listed_agents.is_none() {
                        listed_agents = Some(
                            self.io
                                .call(HerdrOp::List, session, member_deadline)
                                .await
                                .and_then(list_from_envelope),
                        );
                    }
                    presence_for_target_error(
                        member,
                        error,
                        listed_agents.as_ref().expect("list response was populated"),
                    )
                }
                Err(error) => presence_for_error(error),
            };
            observations.push(HerdrMemberPresence {
                ordinal: member.ordinal,
                name: member.name.clone(),
                herdr_agent: Some(member.herdr_agent.clone()),
                outcome,
            });
        }
        observations
    }

    fn state_for_error(
        &self,
        error: HerdrError,
        elapsed: Duration,
        session: Option<&HerdrSession>,
    ) -> HerdrDoctorState {
        if let HerdrError::ServerUnavailable { io_error_kind, .. } = &error
            && let Some(endpoint) = self.io.endpoint_display(session)
        {
            return if *io_error_kind == Some(ErrorKind::PermissionDenied) {
                HerdrDoctorState::PermissionDenied { endpoint }
            } else {
                HerdrDoctorState::EndpointUnreachable { endpoint }
            };
        }
        match error {
            HerdrError::ServerUnavailable {
                io_error_kind: Some(ErrorKind::NotFound),
                ..
            } => HerdrDoctorState::BinaryNotFound {
                searched: self.config.binary_path().map_or_else(
                    || vec![PathBuf::from("herdr")],
                    |path| vec![path.to_path_buf()],
                ),
            },
            HerdrError::ServerUnavailable {
                io_error_kind: Some(ErrorKind::PermissionDenied),
                message,
                ..
            } => HerdrDoctorState::BinaryNotExecutable {
                path: self
                    .config
                    .binary_path()
                    .map_or_else(|| PathBuf::from("herdr"), PathBuf::from),
                cause: if message.is_empty() {
                    "the Herdr binary could not be executed".to_owned()
                } else {
                    message
                },
            },
            HerdrError::ServerUnavailable { .. } if self.config.binary_path().is_some() => {
                HerdrDoctorState::BinaryNotExecutable {
                    path: self
                        .config
                        .binary_path()
                        .expect("checked configured path")
                        .to_path_buf(),
                    cause: "the configured Herdr binary could not be executed".to_owned(),
                }
            }
            HerdrError::ServerUnavailable { .. } => HerdrDoctorState::BinaryNotFound {
                searched: vec![PathBuf::from("herdr")],
            },
            HerdrError::ServerNotRunning => HerdrDoctorState::ServerNotRunning {
                endpoint_named_by_herdr: None,
            },
            HerdrError::ProtocolMismatch { .. } => HerdrDoctorState::ClientServerMismatch {
                client: None,
                server: None,
            },
            HerdrError::Timeout | HerdrError::TimedOut => {
                HerdrDoctorState::ProbeTimedOut { after: elapsed }
            }
            error => unexpected_response(error),
        }
    }
}

fn presence_for_target_error(
    member: &HerdrRosterMember,
    error: HerdrError,
    listed: &Result<HerdrListOutcome, HerdrError>,
) -> HerdrPresenceOutcome {
    let Ok(listed) = listed else {
        return presence_for_error(error);
    };
    if matches!(error, HerdrError::AgentTargetAmbiguous) {
        return presence_for_ambiguous_target(member, listed);
    }
    let unnamed = listed
        .agents
        .iter()
        .filter(|agent| agent.name.is_none())
        .collect::<Vec<_>>();
    if !unnamed.is_empty() {
        let rename_commands = unnamed
            .iter()
            .filter_map(|agent| agent.pane_id.as_deref())
            .map(|pane| format!("herdr agent rename {pane} {}", member.herdr_agent))
            .collect::<Vec<_>>()
            .join("; ");
        return HerdrPresenceOutcome::Finding {
            finding: DoctorFinding {
                severity: DoctorSeverity::Warning,
                code: AtmErrorCode::WarningHerdrUnnamedAgentTarget,
                message: format!(
                    "Herdr target `{}` for member `{}` was not found; {} unnamed agent(s) are present",
                    member.herdr_agent,
                    member.name,
                    unnamed.len()
                ),
                remediation: Some(rename_commands),
            },
        };
    }
    if member.herdr_agent.as_str() != member.name.as_str()
        && let Some(agent) = listed
            .agents
            .iter()
            .find(|agent| agent.name.as_deref() == Some(member.name.as_str()))
    {
        let pane = agent.pane_id.as_deref().unwrap_or("<pane_id>");
        return HerdrPresenceOutcome::Finding {
            finding: DoctorFinding {
                severity: DoctorSeverity::Warning,
                code: AtmError::from(HerdrError::AgentNotFound).code(),
                message: format!(
                    "Herdr member `{}` targets alias `{}`, but Herdr names the agent `{}`",
                    member.name, member.herdr_agent, member.name
                ),
                remediation: Some(format!(
                    "Either run `herdr agent rename {pane} {}` or `atm teams update-member <team> {} --alias {}`",
                    member.herdr_agent, member.name, member.name
                )),
            },
        };
    }
    presence_for_error(HerdrError::AgentNotFound)
}

fn presence_for_ambiguous_target(
    member: &HerdrRosterMember,
    listed: &HerdrListOutcome,
) -> HerdrPresenceOutcome {
    let matches = listed
        .agents
        .iter()
        .filter(|agent| agent.name.as_deref() == Some(member.herdr_agent.as_str()))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return presence_for_error(HerdrError::AgentTargetAmbiguous);
    }
    let agents = matches
        .iter()
        .map(|agent| {
            agent.pane_id.as_deref().map_or_else(
                || member.herdr_agent.to_string(),
                |pane| format!("{} ({pane})", member.herdr_agent),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let remediation = matches
        .iter()
        .filter_map(|agent| agent.pane_id.as_deref())
        .map(|pane| format!("herdr agent rename {pane} --clear"))
        .collect::<Vec<_>>()
        .join("; ");
    HerdrPresenceOutcome::Finding {
        finding: DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmError::from(HerdrError::AgentTargetAmbiguous).code(),
            message: format!(
                "Herdr agent_target_ambiguous for target `{}`; matching agents: {agents}",
                member.herdr_agent
            ),
            remediation: (!remediation.is_empty()).then_some(remediation),
        },
    }
}

fn stale_session_findings(
    session: Option<&HerdrSession>,
    members: &[HerdrRosterMember],
    error: &HerdrError,
) -> Vec<DoctorFinding> {
    if !matches!(
        error,
        HerdrError::ServerNotRunning | HerdrError::ServerUnavailable { .. }
    ) || HerdrSession::named(session).is_none()
        || members.is_empty()
    {
        return Vec::new();
    }
    let members = members
        .iter()
        .map(|member| member.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    vec![DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: AtmErrorCode::HerdrUnavailable,
        message: format!(
            "Herdr session `{}` is unavailable for roster member(s): {members}",
            session.expect("named session was checked")
        ),
        remediation: Some(
            "Run `atm teams update-member <team> <member> --backend herdr` for the default server, or select the correct `--session`".to_owned(),
        ),
    }]
}

fn unexpected_response(error: HerdrError) -> HerdrDoctorState {
    let emission_outcome = error.emission_outcome().to_owned();
    match error {
        HerdrError::Advisory { code, message } => HerdrDoctorState::UnexpectedResponse {
            code: Some(code),
            detail: detail_or_fallback(
                &message,
                "Herdr status query returned an unrecognized advisory response",
            ),
        },
        HerdrError::InternalError { message } => HerdrDoctorState::UnexpectedResponse {
            code: Some(emission_outcome),
            detail: detail_or_fallback(
                &message,
                "Herdr status query did not return a supported response",
            ),
        },
        HerdrError::Unavailable { retry_after } => HerdrDoctorState::UnexpectedResponse {
            code: Some(emission_outcome),
            detail: format!(
                "Herdr reported itself unavailable; retry after {} seconds",
                retry_after.as_secs_f64()
            ),
        },
        _ => HerdrDoctorState::UnexpectedResponse {
            code: Some(emission_outcome),
            detail: "Herdr status query did not return a supported response".to_owned(),
        },
    }
}

fn detail_or_fallback(message: &str, fallback: &str) -> String {
    if message.is_empty() {
        fallback.to_owned()
    } else {
        message.to_owned()
    }
}

fn state_for_server(version: HerdrVersion, protocol: u32) -> HerdrDoctorState {
    let minimum = HerdrVersion::parse(HERDR_MINIMUM_VERSION).expect("minimum version is valid");
    let below_minimum = semver::Version::parse(version.as_str())
        .expect("HerdrVersion guarantees semantic version syntax")
        < semver::Version::parse(minimum.as_str()).expect("minimum version is valid");
    if below_minimum {
        HerdrDoctorState::BelowMinimum { version, minimum }
    } else {
        HerdrDoctorState::Ok { version, protocol }
    }
}

fn remaining_member_deadline(deadline: RequestDeadline) -> RequestDeadline {
    let remaining = deadline.remaining().unwrap_or(Duration::ZERO);
    RequestDeadline::after(remaining.min(Duration::from_secs(2)))
}

fn presence_for_error(error: HerdrError) -> HerdrPresenceOutcome {
    let infrastructure = error.is_infrastructure();
    let outcome = error.diagnostic_name();
    let error: AtmError = error.into();
    if infrastructure {
        HerdrPresenceOutcome::Infrastructure {
            code: error.code(),
            detail: error.detail().to_owned(),
        }
    } else {
        HerdrPresenceOutcome::Finding {
            finding: DoctorFinding {
                severity: DoctorSeverity::Warning,
                code: error.code(),
                message: format!("Herdr {outcome}: {}", error.detail()),
                remediation: Some(error.remediation().to_owned()),
            },
        }
    }
}

fn endpoint_provenance(
    config: &HerdrClientConfig,
    session: Option<&HerdrSession>,
) -> HerdrEndpointProvenance {
    if config.socket_path().is_some() {
        HerdrEndpointProvenance::SocketPath
    } else if HerdrSession::named(session).is_some() {
        HerdrEndpointProvenance::Session
    } else {
        HerdrEndpointProvenance::HerdrDefault
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::{
        HerdrDoctorProbe, endpoint_provenance, presence_for_error, presence_for_target_error,
        stale_session_findings, state_for_server,
    };
    use crate::{AgentSnapshot, HerdrAgentStatus, HerdrClientConfig, HerdrError, HerdrListOutcome};
    use atm_core::doctor::HerdrRosterMember;
    use atm_core::doctor::{
        HerdrDoctorState, HerdrEndpointProvenance, HerdrPresenceOutcome, HerdrVersion,
    };
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::types::AgentName;
    use atm_core::{HerdrAgentName, HerdrSession};

    #[test]
    fn construction_selects_transport_without_running_a_command() {
        let _probe = HerdrDoctorProbe::new(Default::default());
    }

    #[test]
    fn default_session_uses_default_endpoint_provenance() {
        let session = HerdrSession::new("default").expect("valid default session");
        assert_eq!(
            endpoint_provenance(&HerdrClientConfig::default(), Some(&session)),
            HerdrEndpointProvenance::HerdrDefault
        );
    }

    #[test]
    fn configured_socket_path_uses_socket_path_endpoint_provenance() {
        let config = HerdrClientConfig::with_socket_path(PathBuf::from("/configured/herdr.sock"));
        let session = HerdrSession::new("default").expect("valid default session");
        assert_eq!(
            endpoint_provenance(&config, Some(&session)),
            HerdrEndpointProvenance::SocketPath
        );
    }

    #[test]
    fn server_state_retains_compatible_version_and_protocol() {
        assert!(matches!(
            state_for_server(HerdrVersion::parse("0.8.2").expect("version"), 20),
            HerdrDoctorState::Ok { protocol: 20, .. }
        ));
        assert!(matches!(
            state_for_server(HerdrVersion::parse("0.7.9").expect("version"), 20),
            HerdrDoctorState::BelowMinimum { .. }
        ));
    }

    #[test]
    fn every_known_transport_error_maps_to_a_typed_doctor_state() {
        let probe = HerdrDoctorProbe::new(Default::default());
        let errors = vec![
            crate::HerdrError::AgentBlocked,
            crate::HerdrError::AgentNotFound,
            crate::HerdrError::AgentNotReady,
            crate::HerdrError::AgentTargetAmbiguous,
            crate::HerdrError::AgentNotRunning,
            crate::HerdrError::AgentPromptStalled,
            crate::HerdrError::ServerNotRunning,
            crate::HerdrError::ProtocolMismatch {
                message: String::new(),
            },
            crate::HerdrError::Timeout,
            crate::HerdrError::InvalidAgentName,
            crate::HerdrError::EmptyAgentPrompt,
            crate::HerdrError::ServerUnavailable {
                message: "unavailable".to_owned(),
                retry_after: None,
                io_error_kind: None,
            },
            crate::HerdrError::InternalError {
                message: "internal".to_owned(),
            },
            crate::HerdrError::TimedOut,
            crate::HerdrError::Unavailable {
                retry_after: Duration::from_secs(1),
            },
            crate::HerdrError::Advisory {
                code: "future_code".to_owned(),
                message: "future detail".to_owned(),
            },
        ];

        for error in errors {
            assert!(
                !matches!(
                    probe.state_for_error(error, Duration::from_millis(5), None),
                    HerdrDoctorState::Other { .. }
                ),
                "a closed HerdrError variant must not degrade to Other"
            );
        }
        assert!(matches!(
            probe.state_for_error(
                crate::HerdrError::ProtocolMismatch {
                    message: String::new(),
                },
                Duration::ZERO,
                None,
            ),
            HerdrDoctorState::ClientServerMismatch { .. }
        ));
    }

    #[test]
    fn internal_error_preserves_herdr_detail_in_doctor_output() {
        let state = HerdrDoctorProbe::new(Default::default()).state_for_error(
            crate::HerdrError::InternalError {
                message: "Herdr returned a useful internal diagnostic".to_owned(),
            },
            Duration::ZERO,
            None,
        );
        assert!(matches!(
            state,
            HerdrDoctorState::UnexpectedResponse { detail, .. }
                if detail == "Herdr returned a useful internal diagnostic"
        ));
    }

    #[test]
    fn advisory_preserves_herdr_detail_in_doctor_output() {
        let state = HerdrDoctorProbe::new(Default::default()).state_for_error(
            crate::HerdrError::Advisory {
                code: "future_code".to_owned(),
                message: "Herdr returned a useful advisory diagnostic".to_owned(),
            },
            Duration::ZERO,
            None,
        );
        assert!(matches!(
            state,
            HerdrDoctorState::UnexpectedResponse { detail, .. }
                if detail == "Herdr returned a useful advisory diagnostic"
        ));
    }

    #[test]
    fn empty_unexpected_response_details_keep_their_fallbacks() {
        let probe = HerdrDoctorProbe::new(Default::default());
        let cases = [
            (
                crate::HerdrError::InternalError {
                    message: String::new(),
                },
                "Herdr status query did not return a supported response",
            ),
            (
                crate::HerdrError::Advisory {
                    code: "future_code".to_owned(),
                    message: String::new(),
                },
                "Herdr status query returned an unrecognized advisory response",
            ),
        ];

        for (error, expected_detail) in cases {
            assert!(matches!(
                probe.state_for_error(error, Duration::ZERO, None),
                HerdrDoctorState::UnexpectedResponse { detail, .. } if detail == expected_detail
            ));
        }
    }

    #[test]
    fn unavailable_error_reports_its_retry_after_in_doctor_output() {
        let state = HerdrDoctorProbe::new(Default::default()).state_for_error(
            crate::HerdrError::Unavailable {
                retry_after: Duration::from_millis(1500),
            },
            Duration::ZERO,
            None,
        );
        assert!(matches!(
            state,
            HerdrDoctorState::UnexpectedResponse { code, detail }
                if code.as_deref() == Some("breaker_unavailable")
                    && detail == "Herdr reported itself unavailable; retry after 1.5 seconds"
        ));
    }

    #[test]
    fn member_infrastructure_remains_typed() {
        assert!(matches!(
            presence_for_error(crate::HerdrError::ServerNotRunning),
            HerdrPresenceOutcome::Infrastructure { .. }
        ));
    }

    fn roster_member(name: &str, target: &str) -> HerdrRosterMember {
        HerdrRosterMember {
            ordinal: 0,
            name: AgentName::from_validated(name),
            herdr_agent: HerdrAgentName::new(target).expect("valid Herdr target"),
        }
    }

    fn snapshot(name: Option<&str>, pane_id: Option<&str>) -> AgentSnapshot {
        AgentSnapshot {
            name: name.map(str::to_owned),
            pane_id: pane_id.map(str::to_owned),
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }
    }

    #[test]
    fn missing_target_with_unnamed_agent_names_the_pane_rename_remediation() {
        let member = roster_member("alice", "delivery-alice");
        let outcome = presence_for_target_error(
            &member,
            HerdrError::AgentNotFound,
            &Ok(HerdrListOutcome {
                agents: vec![snapshot(None, Some("pane-7"))],
            }),
        );

        assert!(matches!(
            outcome,
            HerdrPresenceOutcome::Finding { finding }
                if finding.code == AtmErrorCode::WarningHerdrUnnamedAgentTarget
                    && finding.message.contains("delivery-alice")
                    && finding.remediation.as_deref()
                        == Some("herdr agent rename pane-7 delivery-alice")
        ));
    }

    #[test]
    fn missing_target_with_canonical_name_reports_alias_mismatch() {
        let member = roster_member("alice", "delivery-alice");
        let outcome = presence_for_target_error(
            &member,
            HerdrError::AgentNotFound,
            &Ok(HerdrListOutcome {
                agents: vec![snapshot(Some("alice"), Some("pane-7"))],
            }),
        );

        assert!(matches!(
            outcome,
            HerdrPresenceOutcome::Finding { finding }
                if finding.code == AtmErrorCode::HerdrAgentNotVisible
                    && finding.message.contains("alias `delivery-alice`")
                    && finding.remediation.as_deref().is_some_and(|value|
                        value.contains("herdr agent rename pane-7 delivery-alice")
                            && value.contains("update-member <team> alice --alias alice"))
        ));
    }

    #[test]
    fn missing_target_without_list_evidence_keeps_the_typed_not_found_finding() {
        let member = roster_member("alice", "delivery-alice");
        let outcome = presence_for_target_error(
            &member,
            HerdrError::AgentNotFound,
            &Ok(HerdrListOutcome {
                agents: vec![snapshot(Some("bob"), Some("pane-8"))],
            }),
        );

        assert!(matches!(
            outcome,
            HerdrPresenceOutcome::Finding { finding }
                if finding.code == AtmErrorCode::HerdrAgentNotVisible
                    && finding.message.contains("agent_not_found")
        ));
    }

    #[test]
    fn ambiguous_target_names_each_stale_pane_for_clear_remediation() {
        let member = roster_member("alice", "delivery-alice");
        let outcome = presence_for_target_error(
            &member,
            HerdrError::AgentTargetAmbiguous,
            &Ok(HerdrListOutcome {
                agents: vec![
                    snapshot(Some("delivery-alice"), Some("pane-7")),
                    snapshot(Some("delivery-alice"), Some("pane-8")),
                ],
            }),
        );

        assert!(matches!(
            outcome,
            HerdrPresenceOutcome::Finding { finding }
                if finding.message.contains("agent_target_ambiguous")
                    && finding.message.contains("pane-7")
                    && finding.message.contains("pane-8")
                    && finding.remediation.as_deref().is_some_and(|value|
                        value.contains("herdr agent rename pane-7 --clear")
                            && value.contains("herdr agent rename pane-8 --clear"))
        ));
    }

    #[test]
    fn stale_named_session_lists_affected_roster_members() {
        let session = HerdrSession::new("stale").expect("valid session");
        let findings = stale_session_findings(
            Some(&session),
            &[roster_member("alice", "alice"), roster_member("bob", "bob")],
            &HerdrError::ServerNotRunning,
        );

        assert!(matches!(
            findings.as_slice(),
            [finding]
                if finding.code == AtmErrorCode::HerdrUnavailable
                    && finding.message.contains("stale")
                    && finding.message.contains("alice, bob")
                    && finding.remediation.as_deref().is_some_and(|value|
                        value.contains("atm teams update-member"))
        ));
    }

    /// Platform-invariant sanitization check: Unix sockets render as
    /// `$HOME/`, `$XDG_CONFIG_HOME/` or `<configured>/`; Windows named pipes
    /// render as `\\.\pipe\%APPDATA%/`, `\\.\pipe\$HOME/` or
    /// `\\.\pipe\<configured>/`.
    fn is_sanitized(endpoint: &atm_core::doctor::HerdrEndpointDisplay) -> bool {
        let display = endpoint
            .as_str()
            .strip_prefix(r"\\.\pipe\")
            .unwrap_or(endpoint.as_str());
        ["$HOME/", "$XDG_CONFIG_HOME/", "%APPDATA%/", "<configured>/"]
            .iter()
            .any(|prefix| display.starts_with(prefix))
    }

    #[test]
    fn socket_unavailable_maps_to_a_sanitized_endpoint() {
        let probe = HerdrDoctorProbe::new(Default::default());
        assert!(matches!(
            probe.state_for_error(
                crate::HerdrError::ServerUnavailable {
                    message: "not found".to_owned(),
                    retry_after: None,
                    io_error_kind: Some(std::io::ErrorKind::NotFound),
                },
                Duration::ZERO,
                None,
            ),
            HerdrDoctorState::EndpointUnreachable { endpoint } if is_sanitized(&endpoint)
        ));
    }

    #[test]
    fn socket_permission_denied_maps_to_a_sanitized_endpoint() {
        let probe = HerdrDoctorProbe::new(Default::default());
        assert!(matches!(
            probe.state_for_error(
                crate::HerdrError::ServerUnavailable {
                    message: "permission denied".to_owned(),
                    retry_after: None,
                    io_error_kind: Some(std::io::ErrorKind::PermissionDenied),
                },
                Duration::ZERO,
                None,
            ),
            HerdrDoctorState::PermissionDenied { endpoint } if is_sanitized(&endpoint)
        ));
    }
}
