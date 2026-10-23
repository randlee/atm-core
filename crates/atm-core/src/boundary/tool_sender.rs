//! Core-owned authority wrapper for a registry-admitted virtual sender.
//!
//! The registry value is inspectable state.  This capability is deliberately
//! neither serializable nor publicly constructible; daemon composition mints
//! it only after registry admission.

use atm_storage::{AgentAddress, TeamName, ToolRuntimeAdmission};

use crate::caller_context::CallerContext;

#[derive(Debug)]
pub struct ToolSenderCapability {
    _verified_runtime_admission: ToolRuntimeAdmission,
}

impl ToolSenderCapability {
    fn from_verified_runtime_admission(admission: ToolRuntimeAdmission) -> (AgentAddress, Self) {
        let sender = admission.sender().address().clone();
        (
            sender,
            Self {
                _verified_runtime_admission: admission,
            },
        )
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
pub fn claim_verified_tool_runtime(admission: ToolRuntimeAdmission) -> SenderClaim {
    let (sender, proof) = ToolSenderCapability::from_verified_runtime_admission(admission);
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
