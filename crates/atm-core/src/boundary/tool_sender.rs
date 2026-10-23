//! Core-owned authority wrapper for a registry-admitted virtual sender.
//!
//! The registry value is inspectable state.  This capability is deliberately
//! neither serializable nor publicly constructible; daemon composition mints
//! it only after registry admission.

use atm_storage::{AgentAddress, MessageDigest, TeamName, ToolPeerAdmission, ToolRuntimeAdmission};

use crate::caller_context::CallerContext;

#[derive(Debug)]
pub struct ToolSenderCapability {
    admission: VerifiedToolAdmission,
}

#[derive(Debug)]
enum VerifiedToolAdmission {
    Local(ToolRuntimeAdmission),
    Peer(ToolPeerAdmission),
}

impl ToolSenderCapability {
    /// Full protected sender identity established by a local or peer admission.
    #[must_use]
    pub fn sender_address(&self) -> &AgentAddress {
        match &self.admission {
            VerifiedToolAdmission::Local(admission) => admission.sender().address(),
            VerifiedToolAdmission::Peer(admission) => admission.sender().address(),
        }
    }

    /// Stable message id bound by the verified signed proof.
    #[must_use]
    pub fn message_id(&self) -> crate::schema::AtmMessageId {
        match &self.admission {
            VerifiedToolAdmission::Local(admission) => admission.message_id(),
            VerifiedToolAdmission::Peer(admission) => admission.message_id(),
        }
    }

    /// Canonical body digest bound by the verified signed proof.
    #[must_use]
    pub fn message_digest(&self) -> &MessageDigest {
        match &self.admission {
            VerifiedToolAdmission::Local(admission) => admission.message_digest(),
            VerifiedToolAdmission::Peer(admission) => admission.message_digest(),
        }
    }

    /// Whether this capability came from the authenticated peer-admission path.
    #[must_use]
    pub fn is_peer_admission(&self) -> bool {
        matches!(&self.admission, VerifiedToolAdmission::Peer(_))
    }

    /// Confirms that the serialized proof is the exact one admitted by its issuer.
    #[must_use]
    pub fn matches_runtime_proof(&self, proof: &atm_storage::ToolRuntimeProof) -> bool {
        matches!(&self.admission, VerifiedToolAdmission::Local(admission) if admission.matches_proof(proof))
    }

    /// Confirms that the serialized attestation is the exact one admitted by its issuer.
    #[must_use]
    pub fn matches_peer_attestation(
        &self,
        attestation: &atm_storage::ToolSenderAttestation,
    ) -> bool {
        matches!(&self.admission, VerifiedToolAdmission::Peer(admission) if admission.matches_attestation(attestation))
    }

    /// Host proven by the peer attestation, if this is a peer admission.
    #[must_use]
    pub fn authenticated_source_host(&self) -> Option<&atm_storage::HostName> {
        match &self.admission {
            VerifiedToolAdmission::Local(_) => None,
            VerifiedToolAdmission::Peer(admission) => Some(admission.authenticated_source_host()),
        }
    }
}

impl From<ToolRuntimeAdmission> for ToolSenderCapability {
    fn from(admission: ToolRuntimeAdmission) -> Self {
        Self {
            admission: VerifiedToolAdmission::Local(admission),
        }
    }
}

impl From<ToolPeerAdmission> for ToolSenderCapability {
    fn from(admission: ToolPeerAdmission) -> Self {
        Self {
            admission: VerifiedToolAdmission::Peer(admission),
        }
    }
}

#[derive(Debug)]
pub enum SenderClaim {
    Ordinary {
        address: AgentAddress,
        caller: CallerContext,
    },
    DaemonTool {
        sender: AgentAddress,
        proof: ToolSenderCapability,
    },
}

/// Converts only a successful registry runtime admission into a daemon tool
/// claim. A resolved [`atm_storage::RegisteredToolSender`] is inspection data
/// and cannot use this seam.
#[must_use]
pub fn claim_verified_tool_runtime(admission: impl Into<ToolSenderCapability>) -> SenderClaim {
    let proof = admission.into();
    let sender = proof.sender_address().clone();
    SenderClaim::DaemonTool { sender, proof }
}

/// Resolve an emission sender only after preserving the provenance that
/// selected it.  A textual `@tool` address is never an ordinary sender: it
/// must arrive as the registry-admitted daemon claim carrying the private
/// capability.  This makes CLI/env/JSON/peer strings unable to impersonate
/// cron or a plug-in sender even when they parse as an [`AgentAddress`].
pub fn resolve_sender_claim(claim: SenderClaim) -> Result<AgentAddress, crate::error::AtmError> {
    match claim {
        SenderClaim::Ordinary { address, caller } => {
            if address.team() == Some(&TeamName::from_validated("tool")) {
                return Err(crate::error::AtmError::validation_with_recovery(
                    "virtual tool senders require daemon registry admission",
                    "use an authenticated ordinary sender or the daemon-owned cron/plugin path",
                ));
            }
            if address.team() != Some(&caller.caller_team)
                || address.agent() != &caller.caller_identity
                || address.chat_id() != caller.caller_chat_id.as_ref()
                || address.host().is_some()
            {
                return Err(crate::error::AtmError::validation_with_recovery(
                    "ordinary sender claim must exactly match the authenticated caller identity",
                    "remove the mismatched sender override or unsupported host qualification",
                ));
            }
            Ok(address)
        }
        SenderClaim::DaemonTool { sender, proof: _ } => Ok(sender),
    }
}

#[cfg(test)]
mod tests {
    use super::{SenderClaim, resolve_sender_claim};
    use crate::caller_context::CallerContext;
    use crate::types::{AgentName, ChatId, TeamName};

    fn caller(chat_id: Option<&str>) -> CallerContext {
        CallerContext {
            caller_identity: AgentName::from_validated("alice"),
            caller_chat_id: chat_id.map(|value| value.parse::<ChatId>().expect("chat id")),
            caller_team: TeamName::from_validated("team"),
            activity_observation: None,
        }
    }

    #[test]
    fn ordinary_sender_requires_authenticated_same_team_identity() {
        let accepted: atm_storage::AgentAddress = "alice@team".parse().expect("address");
        assert_eq!(
            resolve_sender_claim(SenderClaim::Ordinary {
                address: accepted.clone(),
                caller: caller(None)
            })
            .expect("matching ordinary sender"),
            accepted
        );
        for address in ["alice@tool", "bob@team", "alice@other"] {
            assert!(
                resolve_sender_claim(SenderClaim::Ordinary {
                    address: address.parse().expect("address"),
                    caller: caller(None),
                })
                .is_err(),
                "{address} must not become an ordinary emission sender"
            );
        }
    }

    #[test]
    fn ordinary_sender_requires_authenticated_same_chat_identity() {
        let accepted: atm_storage::AgentAddress = "alice:trusted@team".parse().expect("address");
        assert_eq!(
            resolve_sender_claim(SenderClaim::Ordinary {
                address: accepted.clone(),
                caller: caller(Some("trusted")),
            })
            .expect("matching qualified sender"),
            accepted
        );

        for address in ["alice@team", "alice:other@team", "alice:trusted@team.host"] {
            assert!(
                resolve_sender_claim(SenderClaim::Ordinary {
                    address: address.parse().expect("address"),
                    caller: caller(Some("trusted")),
                })
                .is_err(),
                "{address} must not become an ordinary emission sender"
            );
        }
    }

    #[test]
    fn ordinary_sender_rejects_chat_qualification_without_authenticated_chat_identity() {
        let address: atm_storage::AgentAddress = "alice:trusted@team".parse().expect("address");

        assert!(
            resolve_sender_claim(SenderClaim::Ordinary {
                address,
                caller: caller(None),
            })
            .is_err()
        );
    }
}
