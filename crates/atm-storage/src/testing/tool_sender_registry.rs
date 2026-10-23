//! Deterministic in-memory double for [`ToolSenderRegistry`].

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::address::AgentAddress;
use crate::contract::sealed;
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::tool_sender::{
    MessageDigest, PluginSenderId, RegisteredToolSender, RegisteredToolSenderIssuer,
    ToolInstallReceipt, ToolInstallerOperation, ToolInstallerRequest, ToolPrincipal,
    ToolRuntimeAdmission, ToolRuntimeAdmissionIssuer, ToolRuntimeProof, ToolSenderFailure,
    ToolSenderRegistry,
};
use crate::types::IsoTimestamp;

#[derive(Default)]
pub struct InMemoryToolSenderRegistry {
    senders: Mutex<BTreeMap<PluginSenderId, RegisteredToolSender>>,
}

/// Private issuer record used only after this deterministic registry has
/// selected a registered sender for the supplied runtime proof.
struct InMemoryToolRuntimeAdmissionIssuer {
    sender: RegisteredToolSender,
    proof: ToolRuntimeProof,
    message_id: AtmMessageId,
    message_digest: MessageDigest,
}

/// Private, deterministic registry state used only to exercise the
/// inspection projection independently of runtime admission.
struct InMemoryRegisteredToolSenderIssuer {
    principal: ToolPrincipal,
    address: AgentAddress,
}

impl sealed::Sealed for InMemoryRegisteredToolSenderIssuer {}

impl RegisteredToolSenderIssuer for InMemoryRegisteredToolSenderIssuer {
    fn principal(&self) -> &ToolPrincipal {
        &self.principal
    }

    fn address(&self) -> &AgentAddress {
        &self.address
    }
}

impl sealed::Sealed for InMemoryToolRuntimeAdmissionIssuer {}

impl ToolRuntimeAdmissionIssuer for InMemoryToolRuntimeAdmissionIssuer {
    fn sender(&self) -> &RegisteredToolSender {
        &self.sender
    }

    fn proof(&self) -> &ToolRuntimeProof {
        &self.proof
    }

    fn message_id(&self) -> AtmMessageId {
        self.message_id
    }

    fn message_digest(&self) -> &MessageDigest {
        &self.message_digest
    }
}

impl InMemoryToolSenderRegistry {
    fn sender(sender: &PluginSenderId) -> Result<RegisteredToolSender, AtmError> {
        let address: AgentAddress = format!("{}@tool", sender.as_str()).parse()?;
        let issuer = InMemoryRegisteredToolSenderIssuer {
            principal: ToolPrincipal::RegisteredPlugin {
                sender: sender.clone(),
            },
            address,
        };
        Ok(RegisteredToolSender::from_issuer(&issuer))
    }
}

impl sealed::Sealed for InMemoryToolSenderRegistry {}

impl ToolSenderRegistry for InMemoryToolSenderRegistry {
    fn apply_signed(
        &self,
        request: ToolInstallerRequest,
        now: IsoTimestamp,
    ) -> Result<ToolInstallReceipt, AtmError> {
        let mut senders = self
            .senders
            .lock()
            .map_err(|_| AtmError::validation("in-memory tool sender registry lock poisoned"))?;
        match request.operation {
            ToolInstallerOperation::Register | ToolInstallerOperation::Rotate => {
                senders.insert(request.sender.clone(), Self::sender(&request.sender)?);
            }
            ToolInstallerOperation::Unregister => {
                senders.remove(&request.sender);
            }
        }
        Ok(ToolInstallReceipt {
            sender: request.sender,
            registration_generation: request.registration_generation,
            applied_at: now,
        })
    }

    fn admit_signed_message(
        &self,
        proof: ToolRuntimeProof,
        message_id: AtmMessageId,
        digest: MessageDigest,
        _now: IsoTimestamp,
    ) -> Result<ToolRuntimeAdmission, AtmError> {
        let sender = self.resolve(&proof.sender)?;
        ToolRuntimeAdmission::from_issuer(&InMemoryToolRuntimeAdmissionIssuer {
            sender,
            proof,
            message_id,
            message_digest: digest,
        })
    }

    fn resolve(&self, sender: &PluginSenderId) -> Result<RegisteredToolSender, AtmError> {
        self.senders
            .lock()
            .map_err(|_| AtmError::validation("in-memory tool sender registry lock poisoned"))?
            .get(sender)
            .cloned()
            .ok_or_else(|| {
                ToolSenderFailure::SenderUnregistered
                    .into_error(format!("tool sender {sender} is not registered"))
            })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::error::AtmErrorCode;
    use crate::tool_sender::{PluginSenderId, ToolKeyId, ToolNonce, ToolSenderRegistry};
    use crate::types::HostName;

    #[test]
    fn unregistered_sender_uses_stable_failure_code() {
        let registry = InMemoryToolSenderRegistry::default();
        let sender = PluginSenderId::parse("missing-plugin").expect("valid fixture sender");

        let error = registry
            .resolve(&sender)
            .expect_err("sender is not registered");

        assert_eq!(error.code(), AtmErrorCode::ToolSenderUnregistered);
        assert!(error.message().contains("authenticated installer"));
    }

    #[test]
    fn issuer_rechecks_the_proof_message_binding() {
        let sender_id = PluginSenderId::parse("plugin").expect("fixed sender");
        let sender = InMemoryToolSenderRegistry::sender(&sender_id).expect("registered sender");
        let proof_message_id =
            AtmMessageId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("fixed message id");
        let issued_message_id =
            AtmMessageId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAW").expect("different message id");
        let digest = MessageDigest::parse("message-digest").expect("fixed digest");
        let proof = ToolRuntimeProof {
            runtime_key_id: ToolKeyId::parse("runtime-key").expect("fixed key"),
            sender: sender_id,
            registration_generation: 1,
            host_id: HostName::from_str("host.example").expect("fixed host"),
            message_id: proof_message_id,
            message_digest: digest.clone(),
            nonce: ToolNonce::parse("message-nonce").expect("fixed nonce"),
            signature: "opaque-signature".into(),
        };

        let error = ToolRuntimeAdmission::from_issuer(&InMemoryToolRuntimeAdmissionIssuer {
            sender,
            proof,
            message_id: issued_message_id,
            message_digest: digest,
        })
        .expect_err("an issuer cannot detach a proof from its message");

        assert!(
            error
                .message()
                .contains("does not bind the requested message id and digest")
        );
    }
}
