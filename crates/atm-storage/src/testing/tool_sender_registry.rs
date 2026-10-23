//! Deterministic in-memory double for [`ToolSenderRegistry`].

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::address::AgentAddress;
use crate::contract::sealed;
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::tool_sender::{
    MessageDigest, PluginSenderId, RegisteredToolSender, ToolInstallReceipt,
    ToolInstallerOperation, ToolInstallerRequest, ToolPrincipal, ToolRuntimeProof,
    ToolSenderRegistry,
};
use crate::types::IsoTimestamp;

#[derive(Default)]
pub struct InMemoryToolSenderRegistry {
    senders: Mutex<BTreeMap<PluginSenderId, RegisteredToolSender>>,
}

impl InMemoryToolSenderRegistry {
    fn sender(sender: &PluginSenderId) -> Result<RegisteredToolSender, AtmError> {
        let address: AgentAddress = format!("{}@tool", sender.as_str()).parse()?;
        Ok(RegisteredToolSender::admitted(
            ToolPrincipal::RegisteredPlugin {
                sender: sender.clone(),
            },
            address,
        ))
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
    ) -> Result<RegisteredToolSender, AtmError> {
        if proof.message_id != message_id || proof.message_digest != digest {
            return Err(AtmError::validation_with_recovery(
                "tool runtime proof does not bind the requested message id and digest",
                "submit the original proof and canonical message unchanged",
            ));
        }
        self.resolve(&proof.sender)
    }

    fn resolve(&self, sender: &PluginSenderId) -> Result<RegisteredToolSender, AtmError> {
        self.senders
            .lock()
            .map_err(|_| AtmError::validation("in-memory tool sender registry lock poisoned"))?
            .get(sender)
            .cloned()
            .ok_or_else(|| {
                AtmError::validation_with_recovery(
                    format!("tool sender {sender} is not registered"),
                    "install the plug-in through an authenticated installer",
                )
            })
    }
}
