//! Concrete endpoint diagnostics over the private Herdr transport seam.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use atm_core::doctor::{
    DoctorFinding, DoctorSeverity, HerdrBinaryProvenance, HerdrBinaryResolution, HerdrDoctorState,
    HerdrEndpointObservation, HerdrEndpointProvenance, HerdrMemberPresence, HerdrPresenceOutcome,
    HerdrRosterMember, HerdrTransportKind, HerdrVersion,
};
use atm_core::error::AtmError;
use atm_core::{HerdrSession, RequestDeadline};

use crate::transport::{HerdrIo, HerdrOp, get_from_envelope, server_status_from_envelope};
use crate::{HERDR_MINIMUM_VERSION, HerdrClientConfig, HerdrError};

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
            provenance: if session.is_some() {
                HerdrEndpointProvenance::Session
            } else {
                HerdrEndpointProvenance::HerdrDefault
            },
            transport: HerdrTransportKind::Cli,
            endpoint: None,
            binary: self.binary_resolution(),
            state: HerdrDoctorState::NotConfigured,
            live_handoff: None,
            members: Vec::new(),
        };

        let status = self
            .io
            .call(HerdrOp::StatusServer, session, deadline)
            .await
            .and_then(server_status_from_envelope);
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                observation.state = self.state_for_error(error, started.elapsed());
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
        for member in members {
            let member_deadline = remaining_member_deadline(deadline);
            let outcome = self
                .io
                .call(
                    HerdrOp::Get {
                        agent: &member.name,
                    },
                    session,
                    member_deadline,
                )
                .await
                .and_then(get_from_envelope)
                .map(|_| HerdrPresenceOutcome::Visible)
                .unwrap_or_else(presence_for_error);
            observations.push(HerdrMemberPresence {
                ordinal: member.ordinal,
                name: member.name.clone(),
                outcome,
            });
        }
        observations
    }

    fn state_for_error(&self, error: HerdrError, elapsed: Duration) -> HerdrDoctorState {
        match error {
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
            HerdrError::Timeout | HerdrError::TimedOut => {
                HerdrDoctorState::ProbeTimedOut { after: elapsed }
            }
            error => HerdrDoctorState::UnexpectedResponse {
                code: Some(error.emission_outcome().to_owned()),
                detail: "Herdr status query did not return a supported response".to_owned(),
            },
        }
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
                message: error.detail().to_owned(),
                remediation: Some(error.remediation().to_owned()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HerdrDoctorProbe, presence_for_error, state_for_server};
    use atm_core::doctor::{HerdrDoctorState, HerdrPresenceOutcome, HerdrVersion};

    #[test]
    fn construction_selects_transport_without_running_a_command() {
        let _probe = HerdrDoctorProbe::new(Default::default());
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
    fn member_infrastructure_remains_typed() {
        assert!(matches!(
            presence_for_error(crate::HerdrError::ServerNotRunning),
            HerdrPresenceOutcome::Infrastructure { .. }
        ));
    }
}
