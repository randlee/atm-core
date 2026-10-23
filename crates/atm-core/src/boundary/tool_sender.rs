//! Core-owned authority wrapper for a registry-admitted virtual sender.
//!
//! The registry value is inspectable state.  This capability is deliberately
//! neither serializable nor publicly constructible; daemon composition mints
//! it only after registry admission.

use atm_storage::{AgentAddress, RegisteredToolSender, TeamName};

use crate::caller_context::CallerContext;

#[derive(Debug)]
pub struct ToolSenderCapability {
    _private: (),
}

impl ToolSenderCapability {
    #[allow(dead_code)]
    pub(crate) fn admitted_by_daemon() -> Self {
        Self { _private: () }
    }
}

#[derive(Debug)]
pub enum SenderClaim {
    Ordinary {
        address: AgentAddress,
        caller: CallerContext,
    },
    DaemonTool {
        sender: RegisteredToolSender,
        proof: ToolSenderCapability,
    },
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
            {
                return Err(crate::error::AtmError::validation_with_recovery(
                    "ordinary sender claim must match the authenticated caller",
                    "remove the mismatched sender override",
                ));
            }
            Ok(address)
        }
        SenderClaim::DaemonTool { sender, proof: _ } => Ok(sender.address().clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::{SenderClaim, resolve_sender_claim};
    use crate::caller_context::CallerContext;
    use crate::types::{AgentName, TeamName};

    fn caller() -> CallerContext {
        CallerContext {
            caller_identity: AgentName::from_validated("alice"),
            caller_chat_id: None,
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
                caller: caller()
            })
            .expect("matching ordinary sender"),
            accepted
        );
        for address in ["alice@tool", "bob@team", "alice@other"] {
            assert!(
                resolve_sender_claim(SenderClaim::Ordinary {
                    address: address.parse().expect("address"),
                    caller: caller(),
                })
                .is_err(),
                "{address} must not become an ordinary emission sender"
            );
        }
    }
}
